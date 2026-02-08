use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogRotation {
    Hourly { compress: bool },
    Daily { compress: bool },
    LineBased { max_lines: usize, compress: bool },
    SizeBased { max_size_mb: u64, compress: bool },
    Never,
}

impl LogRotation {
    pub fn should_compress(&self) -> bool {
        match self {
            Self::Hourly { compress } => *compress,
            Self::Daily { compress } => *compress,
            Self::LineBased { compress, .. } => *compress,
            Self::SizeBased { compress, .. } => *compress,
            Self::Never => false,
        }
    }
}

pub struct RotationWriter {
    base_path: PathBuf,
    strategy: LogRotation,
    current_file: Arc<Mutex<Option<BufWriter<File>>>>,
    current_lines: Arc<AtomicUsize>,
    current_size: Arc<AtomicUsize>,
    current_index: Arc<AtomicUsize>,
    last_rotation_time: Arc<Mutex<SystemTime>>,
    current_time_bucket: Arc<Mutex<TimeRotationBucket>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeRotationBucket {
    hour: u32,
    day: u32,
}

impl TimeRotationBucket {
    fn now_hourly() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let hours = (now.as_secs() / 3600) as u32;
        Self { hour: hours, day: 0 }
    }

    fn now_daily() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let days = (now.as_secs() / 86400) as u32;
        Self { hour: 0, day: days }
    }

    fn empty() -> Self {
        Self { hour: 0, day: 0 }
    }
}

impl RotationWriter {
    pub fn new(base_path: PathBuf, strategy: LogRotation) -> std::io::Result<Self> {
        if let Some(parent) = base_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let initial_bucket = match &strategy {
            LogRotation::Hourly { .. } => TimeRotationBucket::now_hourly(),
            LogRotation::Daily { .. } => TimeRotationBucket::now_daily(),
            _ => TimeRotationBucket::empty(),
        };

        Ok(Self {
            base_path,
            strategy,
            current_file: Arc::new(Mutex::new(None)),
            current_lines: Arc::new(AtomicUsize::new(0)),
            current_size: Arc::new(AtomicUsize::new(0)),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_rotation_time: Arc::new(Mutex::new(SystemTime::now())),
            current_time_bucket: Arc::new(Mutex::new(initial_bucket)),
        })
    }

    pub async fn write_line(&self, line: &str) -> std::io::Result<()> {
        let should_rotate = self.should_rotate().await?;
        
        if should_rotate {
            self.rotate().await?;
        }

        let mut file_guard = self.current_file.lock().await;
        
        if file_guard.is_none() {
            *file_guard = Some(self.open_current_file()?);
        }

        if let Some(ref mut writer) = *file_guard {
            writeln!(writer, "{}", line)?;
            writer.flush()?;
            
            let line_len = line.len() + 1;
            self.current_lines.fetch_add(1, Ordering::SeqCst);
            self.current_size.fetch_add(line_len, Ordering::SeqCst);
        }

        Ok(())
    }

    async fn should_rotate(&self) -> std::io::Result<bool> {
        match &self.strategy {
            LogRotation::LineBased { max_lines, .. } => {
                Ok(self.current_lines.load(Ordering::SeqCst) >= *max_lines)
            }
            LogRotation::SizeBased { max_size_mb, .. } => {
                let max_bytes = max_size_mb * 1024 * 1024;
                Ok(self.current_size.load(Ordering::SeqCst) >= max_bytes as usize)
            }
            LogRotation::Hourly { .. } => {
                let current_bucket = TimeRotationBucket::now_hourly();
                let stored_bucket = self.current_time_bucket.lock().await;
                Ok(current_bucket != *stored_bucket)
            }
            LogRotation::Daily { .. } => {
                let current_bucket = TimeRotationBucket::now_daily();
                let stored_bucket = self.current_time_bucket.lock().await;
                Ok(current_bucket != *stored_bucket)
            }
            LogRotation::Never => Ok(false),
        }
    }

    async fn rotate(&self) -> std::io::Result<()> {
        let mut file_guard = self.current_file.lock().await;
        
        if let Some(mut writer) = file_guard.take() {
            writer.flush()?;
            drop(writer);
        }

        let old_index = self.current_index.load(Ordering::SeqCst);
        let old_file = self.get_file_path(old_index);

        if old_file.exists() && self.strategy.should_compress() {
            let compressed_path = self.get_compressed_path(old_index);
            compress_file(&old_file, &compressed_path)?;
        }

        self.current_index.fetch_add(1, Ordering::SeqCst);
        self.current_lines.store(0, Ordering::SeqCst);
        self.current_size.store(0, Ordering::SeqCst);
        *self.last_rotation_time.lock().await = SystemTime::now();
        
        match &self.strategy {
            LogRotation::Hourly { .. } => {
                *self.current_time_bucket.lock().await = TimeRotationBucket::now_hourly();
            }
            LogRotation::Daily { .. } => {
                *self.current_time_bucket.lock().await = TimeRotationBucket::now_daily();
            }
            _ => {}
        }
        
        *file_guard = Some(self.open_current_file()?);

        Ok(())
    }

    fn open_current_file(&self) -> std::io::Result<BufWriter<File>> {
        let index = self.current_index.load(Ordering::SeqCst);
        let path = self.get_file_path(index);
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        
        Ok(BufWriter::new(file))
    }

    fn get_file_path(&self, index: usize) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let file_name = match &self.strategy {
            LogRotation::Hourly { .. } | LogRotation::Daily { .. } => {
                format!(
                    "{}.{}.{:04}.log",
                    self.base_path.file_stem().unwrap().to_string_lossy(),
                    timestamp,
                    index
                )
            }
            _ => {
                format!(
                    "{}.{:04}.log",
                    self.base_path.file_stem().unwrap().to_string_lossy(),
                    index
                )
            }
        };
        
        self.base_path.with_file_name(file_name)
    }

    fn get_compressed_path(&self, index: usize) -> PathBuf {
        let file_path = self.get_file_path(index);
        file_path.with_extension("log.gz")
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        let mut file_guard = self.current_file.lock().await;
        if let Some(ref mut writer) = *file_guard {
            writer.flush()?;
        }
        Ok(())
    }

    pub fn list_log_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let parent = self.base_path.parent().unwrap_or(Path::new("."));
        let stem = self.base_path.file_stem().unwrap().to_string_lossy();
        
        let mut files = Vec::new();
        
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().unwrap().to_string_lossy();
            
            if file_name.starts_with(stem.as_ref()) && 
               (file_name.ends_with(".log") || file_name.ends_with(".log.gz")) {
                files.push(path);
            }
        }
        
        files.sort();
        Ok(files)
    }

    pub fn get_stats(&self) -> RotationStats {
        RotationStats {
            current_lines: self.current_lines.load(Ordering::SeqCst),
            current_size_bytes: self.current_size.load(Ordering::SeqCst),
            current_index: self.current_index.load(Ordering::SeqCst),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RotationStats {
    pub current_lines: usize,
    pub current_size_bytes: usize,
    pub current_index: usize,
}

fn compress_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let input = fs::read(src)?;
    let output_file = File::create(dst)?;
    let mut encoder = GzEncoder::new(output_file, Compression::default());
    encoder.write_all(&input)?;
    encoder.finish()?;
    fs::remove_file(src)?;
    Ok(())
}

pub struct TracingRotationWriter {
    writer: Arc<RotationWriter>,
    runtime: tokio::runtime::Runtime,
}

impl TracingRotationWriter {
    pub fn new(writer: Arc<RotationWriter>) -> Self {
        let runtime = tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime for log rotation");
        Self { writer, runtime }
    }
}

impl std::io::Write for TracingRotationWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let line = String::from_utf8_lossy(buf);
        let line = line.trim_end_matches('\n');
        
        self.runtime.block_on(async {
            self.writer.write_line(line).await
        })?;
        
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.runtime.block_on(async {
            self.writer.flush().await
        })
    }
}

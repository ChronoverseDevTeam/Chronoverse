use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct LineBasedRotation {
    base_path: PathBuf,
    max_lines_per_file: usize,
    current_file: Arc<Mutex<Option<BufWriter<File>>>>,
    current_lines: Arc<AtomicUsize>,
    current_index: Arc<AtomicUsize>,
    compress_old: bool,
}

impl LineBasedRotation {
    pub fn new(base_path: PathBuf, max_lines_per_file: usize, compress_old: bool) -> std::io::Result<Self> {
        if let Some(parent) = base_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let rotation = Self {
            base_path,
            max_lines_per_file,
            current_file: Arc::new(Mutex::new(None)),
            current_lines: Arc::new(AtomicUsize::new(0)),
            current_index: Arc::new(AtomicUsize::new(0)),
            compress_old,
        };

        Ok(rotation)
    }

    pub async fn write_line(&self, line: &str) -> std::io::Result<()> {
        let current_lines = self.current_lines.load(Ordering::SeqCst);
        
        if current_lines >= self.max_lines_per_file {
            self.rotate().await?;
        }

        let mut file_guard = self.current_file.lock().await;
        
        if file_guard.is_none() {
            *file_guard = Some(self.open_current_file()?);
        }

        if let Some(ref mut writer) = *file_guard {
            writeln!(writer, "{}", line)?;
            writer.flush()?;
            self.current_lines.fetch_add(1, Ordering::SeqCst);
        }

        Ok(())
    }

    async fn rotate(&self) -> std::io::Result<()> {
        let mut file_guard = self.current_file.lock().await;
        
        if let Some(mut writer) = file_guard.take() {
            writer.flush()?;
            drop(writer);
        }

        let old_index = self.current_index.load(Ordering::SeqCst);
        let old_file = self.get_file_path(old_index);

        if old_file.exists() && self.compress_old {
            let compressed_path = self.get_compressed_path(old_index);
            tokio::task::spawn_blocking(move || {
                compress_file(&old_file, &compressed_path)
            }).await??;
        }

        self.current_index.fetch_add(1, Ordering::SeqCst);
        self.current_lines.store(0, Ordering::SeqCst);
        
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
        let file_name = format!(
            "{}.{:04}.log",
            self.base_path.file_stem().unwrap().to_string_lossy(),
            index
        );
        self.base_path.with_file_name(file_name)
    }

    fn get_compressed_path(&self, index: usize) -> PathBuf {
        let file_name = format!(
            "{}.{:04}.log.gz",
            self.base_path.file_stem().unwrap().to_string_lossy(),
            index
        );
        self.base_path.with_file_name(file_name)
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
}

fn compress_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    
    let input = fs::read(src)?;
    let output_file = File::create(dst)?;
    let mut encoder = GzEncoder::new(output_file, Compression::default());
    encoder.write_all(&input)?;
    encoder.finish()?;
    
    fs::remove_file(src)?;
    
    Ok(())
}


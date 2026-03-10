use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry {
    Begin { tx_id: u64, timestamp: u64 },
    Write { tx_id: u64, key: String, value: String },
    Commit { tx_id: u64, timestamp: u64 },
    Abort { tx_id: u64, timestamp: u64 },
    Checkpoint { timestamp: u64 },
}

impl LogEntry {
    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn from_string(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

pub struct RecoveryLog {
    log_path: PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
    next_tx_id: Arc<Mutex<u64>>,
}

impl RecoveryLog {
    pub fn new(log_path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let writer = Arc::new(Mutex::new(BufWriter::new(file)));
        
        let next_tx_id = Arc::new(Mutex::new(Self::scan_last_tx_id(&log_path)? + 1));

        Ok(Self {
            log_path,
            writer,
            next_tx_id,
        })
    }

    fn scan_last_tx_id(log_path: &Path) -> std::io::Result<u64> {
        if !log_path.exists() {
            return Ok(0);
        }

        let file = File::open(log_path)?;
        let reader = BufReader::new(file);
        let mut max_tx_id = 0u64;

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(entry) = LogEntry::from_string(&line) {
                    let tx_id = match entry {
                        LogEntry::Begin { tx_id, .. } => tx_id,
                        LogEntry::Write { tx_id, .. } => tx_id,
                        LogEntry::Commit { tx_id, .. } => tx_id,
                        LogEntry::Abort { tx_id, .. } => tx_id,
                        LogEntry::Checkpoint { .. } => continue,
                    };
                    max_tx_id = max_tx_id.max(tx_id);
                }
            }
        }

        Ok(max_tx_id)
    }

    pub async fn allocate_tx_id(&self) -> u64 {
        let mut tx_id = self.next_tx_id.lock().await;
        let id = *tx_id;
        *tx_id += 1;
        id
    }

    pub async fn write_entry(&self, entry: &LogEntry) -> std::io::Result<()> {
        let mut writer = self.writer.lock().await;
        writeln!(writer, "{}", entry.to_string())?;
        writer.flush()?;
        Ok(())
    }

    pub async fn begin_transaction(&self) -> std::io::Result<u64> {
        let tx_id = self.allocate_tx_id().await;
        let entry = LogEntry::Begin {
            tx_id,
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry).await?;
        Ok(tx_id)
    }

    pub async fn write_data(&self, tx_id: u64, key: String, value: String) -> std::io::Result<()> {
        let entry = LogEntry::Write { tx_id, key, value };
        self.write_entry(&entry).await
    }

    pub async fn commit_transaction(&self, tx_id: u64) -> std::io::Result<()> {
        let entry = LogEntry::Commit {
            tx_id,
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry).await
    }

    pub async fn abort_transaction(&self, tx_id: u64) -> std::io::Result<()> {
        let entry = LogEntry::Abort {
            tx_id,
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry).await
    }

    pub async fn checkpoint(&self) -> std::io::Result<()> {
        let entry = LogEntry::Checkpoint {
            timestamp: Self::current_timestamp(),
        };
        self.write_entry(&entry).await
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn recover(&self) -> std::io::Result<RecoveryState> {
        let mut state = RecoveryState::default();

        if !self.log_path.exists() {
            return Ok(state);
        }

        let file = File::open(&self.log_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = LogEntry::from_string(&line) {
                state.process_entry(entry);
            }
        }

        state.finalize();
        Ok(state)
    }
}

#[derive(Debug, Default)]
pub struct RecoveryState {
    pub committed_data: std::collections::HashMap<String, String>,
    pub active_transactions: std::collections::HashSet<u64>,
    pub transaction_data: std::collections::HashMap<u64, Vec<(String, String)>>,
    pub last_checkpoint: Option<u64>,
}

impl RecoveryState {
    fn process_entry(&mut self, entry: LogEntry) {
        match entry {
            LogEntry::Begin { tx_id, .. } => {
                self.active_transactions.insert(tx_id);
                self.transaction_data.insert(tx_id, Vec::new());
            }
            LogEntry::Write { tx_id, key, value } => {
                if let Some(data) = self.transaction_data.get_mut(&tx_id) {
                    data.push((key, value));
                }
            }
            LogEntry::Commit { tx_id, .. } => {
                if let Some(data) = self.transaction_data.remove(&tx_id) {
                    for (key, value) in data {
                        self.committed_data.insert(key, value);
                    }
                }
                self.active_transactions.remove(&tx_id);
            }
            LogEntry::Abort { tx_id, .. } => {
                self.transaction_data.remove(&tx_id);
                self.active_transactions.remove(&tx_id);
            }
            LogEntry::Checkpoint { timestamp } => {
                self.last_checkpoint = Some(timestamp);
            }
        }
    }

    fn finalize(&mut self) {
        for tx_id in self.active_transactions.iter() {
            self.transaction_data.remove(tx_id);
        }
    }

    pub fn get_value(&self, key: &str) -> Option<&String> {
        self.committed_data.get(key)
    }

    pub fn has_uncommitted(&self) -> bool {
        !self.active_transactions.is_empty()
    }
}


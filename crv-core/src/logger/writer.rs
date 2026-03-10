use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::io::AsyncWriteExt;

pub(crate) enum LogMessage {
    Write(String),
    Flush,
}

pub struct LogWriter {
    tx: mpsc::UnboundedSender<LogMessage>,
    _handle: tokio::task::JoinHandle<()>,
}

impl LogWriter {
    pub fn new(file_path: PathBuf) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        
        let handle = tokio::spawn(async move {
            if let Err(e) = Self::write_loop(file_path, rx).await {
                eprintln!("Log writer error: {}", e);
            }
        });

        Ok(Self {
            tx,
            _handle: handle,
        })
    }

    pub fn write(&self, message: String) -> Result<(), String> {
        self.tx.send(LogMessage::Write(message))
            .map_err(|e| e.to_string())
    }

    pub fn flush(&self) -> Result<(), String> {
        self.tx.send(LogMessage::Flush)
            .map_err(|e| e.to_string())
    }

    async fn write_loop(
        file_path: PathBuf,
        mut rx: mpsc::UnboundedReceiver<LogMessage>,
    ) -> std::io::Result<()> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        let mut writer = tokio::io::BufWriter::new(file);

        while let Some(message) = rx.recv().await {
            match message {
                LogMessage::Write(content) => {
                    writer.write_all(content.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                LogMessage::Flush => {
                    writer.flush().await?;
                }
            }
        }

        writer.flush().await?;
        Ok(())
    }
}


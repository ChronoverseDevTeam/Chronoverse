use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use flate2::read::GzDecoder;

pub struct LogReader {
    files: Vec<PathBuf>,
}

impl LogReader {
    pub fn new(log_dir: &Path, pattern: &str) -> std::io::Result<Self> {
        let mut files = Vec::new();
        
        for entry in std::fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().unwrap().to_string_lossy();
            
            if file_name.contains(pattern) {
                files.push(path);
            }
        }
        
        files.sort();
        
        Ok(Self { files })
    }

    pub fn read_all_lines(&self) -> std::io::Result<Vec<String>> {
        let mut all_lines = Vec::new();
        
        for file_path in &self.files {
            let lines = self.read_file_lines(file_path)?;
            all_lines.extend(lines);
        }
        
        Ok(all_lines)
    }

    pub fn read_file_lines(&self, path: &Path) -> std::io::Result<Vec<String>> {
        let mut lines = Vec::new();
        
        if path.extension().and_then(|s| s.to_str()) == Some("gz") {
            let file = File::open(path)?;
            let decoder = GzDecoder::new(file);
            let reader = BufReader::new(decoder);
            
            for line in reader.lines() {
                lines.push(line?);
            }
        } else {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            
            for line in reader.lines() {
                lines.push(line?);
            }
        }
        
        Ok(lines)
    }

    pub fn count_lines(&self) -> std::io::Result<usize> {
        let mut count = 0;
        
        for file_path in &self.files {
            count += self.count_file_lines(file_path)?;
        }
        
        Ok(count)
    }

    fn count_file_lines(&self, path: &Path) -> std::io::Result<usize> {
        let lines = self.read_file_lines(path)?;
        Ok(lines.len())
    }

    pub fn list_files(&self) -> &[PathBuf] {
        &self.files
    }
}


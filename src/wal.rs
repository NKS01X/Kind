use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WalCommand {
    Put { key: String, value: Vec<u8>, #[serde(default)] expires_at: Option<u64> },
    Delete { key: String },
    TxCommit(Vec<WalCommand>),
}

pub struct Wal {
    file: File,
}

impl Wal {
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        
        Ok(Self { file })
    }

    pub fn append(&mut self, command: &WalCommand) -> std::io::Result<()> {
        let mut json = serde_json::to_string(command)?;
        json.push('\n');
        self.file.write_all(json.as_bytes())?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn truncate(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        Ok(())
    }

    pub fn read_all_commands<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<WalCommand>> {
        if !path.as_ref().exists() {
            return Ok(Vec::new());
        }
        
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut commands = Vec::new();
        
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(cmd) = serde_json::from_str(&line) {
                commands.push(cmd);
            }
        }
        
        Ok(commands)
    }
}

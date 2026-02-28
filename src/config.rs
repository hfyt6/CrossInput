use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use anyhow::Result;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SlaveConnection {
    pub address: String,
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub ip: String,
    pub port: u16,
    pub slave_connections: Vec<SlaveConnection>,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        
        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }

    pub fn default_master_config() -> Self {
        Config {
            ip: "0.0.0.0".to_string(),
            port: 8080,
            slave_connections: vec![],
        }
    }
}
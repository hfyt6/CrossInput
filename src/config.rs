use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use anyhow::Result;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub shared_key: String,
    pub ip: String,
    pub port: u16,
    pub slave_addresses: Vec<String>,
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
            shared_key: "default_key".to_string(),
            ip: "0.0.0.0".to_string(),
            port: 8080,
            slave_addresses: vec![],
        }
    }

    pub fn default_slave_config() -> Self {
        Config {
            shared_key: "default_key".to_string(),
            ip: "0.0.0.0".to_string(),
            port: 8081, // Default port for slave to listen on
            slave_addresses: vec![],
        }
    }
}
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce, Key,
};
use rand::RngCore;
use anyhow::Result;

/// Manages encryption and decryption of data
pub struct EncryptionManager {
    cipher: Aes256Gcm,
    key_str: String,
}

impl EncryptionManager {
    /// Creates a new encryption manager with the given key
    pub fn new(key_str: &str) -> Result<Self> {
        // Create a key from the provided string (pad or hash to correct length)
        let mut key_bytes = [0u8; 32]; // Aes256 requires 32-byte keys
        let key_bytes_source = key_str.as_bytes();
        
        // Fill the key with the provided string, repeating if necessary
        for (i, byte) in key_bytes_source.iter().cycle().take(32).enumerate() {
            key_bytes[i] = *byte;
        }
        
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        
        Ok(EncryptionManager {
            cipher,
            key_str: key_str.to_string(),
        })
    }

    /// Encrypts the provided data
    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Generate a random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Encrypt the data
        let ciphertext = self.cipher
            .encrypt(nonce, data)
            .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;
        
        // Prepend the nonce to the ciphertext
        let mut result = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        
        Ok(result)
    }

    /// Decrypts the provided data
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(anyhow::anyhow!("Encrypted data too short"));
        }
        
        // Extract the nonce from the beginning
        let nonce_bytes = &data[..12];
        let nonce = Nonce::from_slice(nonce_bytes);
        
        // Extract the ciphertext
        let ciphertext = &data[12..];
        
        // Decrypt the data
        let plaintext = self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        
        Ok(plaintext)
    }
}

impl Clone for EncryptionManager {
    fn clone(&self) -> Self {
        EncryptionManager::new(&self.key_str).expect("Failed to clone encryption manager")
    }
}

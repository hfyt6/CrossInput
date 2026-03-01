use crate::MessageType;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// Message queue functionality for MasterClient
impl super::MasterClient {
    /// Process the message queue in a background thread and send messages to all slaves
    pub async fn process_message_queue(&self) -> Result<()> {
        println!("Starting message queue processor...");
        
        loop {
            // Process all messages in the queue
            let mut messages_to_process = Vec::new();
            {
                let mut queue = self.key_event_queue.lock().await;
                while let Some(message) = queue.pop_front() {
                    messages_to_process.push(message);
                }
            }
            
            // Send all queued messages to slaves
            for message in messages_to_process {
                // println!("Processing message from queue: {:?}", message);
                
                // Send the message to all connected slaves
                if let Err(e) = self.broadcast_to_all_slaves(message).await {
                    eprintln!("Error broadcasting message to slaves: {}", e);
                } else {
                    // println!("Message successfully sent to all slaves");
                }
            }
            
            // Sleep briefly before checking the queue again (every 10ms as required)
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Add a message to the key event queue
    pub async fn add_to_key_event_queue(&self, message: MessageType) -> Result<()> {
        // println!("Adding message to key event queue: {:?}", message);
        
        // Add the message to the queue
        {
            let mut queue = self.key_event_queue.lock().await;
            queue.push_back(message);
        }
        
        Ok(())
    }
}
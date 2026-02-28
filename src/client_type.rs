/// Defines the role of the client in the network
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientRole {
    /// Master client that initiates connections and sends data
    Master,
    /// Slave client that accepts connections and receives data
    Slave,
}

/// Defines the type of client connection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientType {
    /// Connection from a master client
    Master,
    /// Connection from a slave client
    Slave,
}
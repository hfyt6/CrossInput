use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use rdev::{EventType, Key};

mod client_type;
mod config;
mod encryption;
mod error;
mod master;
mod slave;

use client_type::ClientRole;
use master::MasterClient;
use slave::SlaveClient;

/// Mouse event types for communication
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MouseEvent {
    /// Mouse button pressed
    ButtonPress { button: MouseButton },
    /// Mouse button released
    ButtonRelease { button: MouseButton },
    /// Mouse moved to position
    Move { x: i32, y: i32 },
    /// Mouse wheel scrolled
    Scroll { delta_x: i32, delta_y: i32 },
}

/// Mouse button types
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Unknown(u16), // For other buttons - using u16 to match rdev::Button size
}

/// Keyboard event types for communication
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KeyboardEvent {
    /// Key pressed
    Press { key: SerializableKey },
    /// Key released
    Release { key: SerializableKey },
}

/// Serializable wrapper for Key to handle serialization
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SerializableKey {
    Alt,
    AltGr,
    Backspace,
    CapsLock,
    ControlLeft,
    ControlRight,
    Delete,
    DownArrow,
    End,
    Escape,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Home,
    LeftArrow,
    MetaLeft,
    MetaRight,
    PageDown,
    PageUp,
    Return,
    RightArrow,
    ShiftLeft,
    ShiftRight,
    Space,
    Tab,
    UpArrow,
    PrintScreen,
    ScrollLock,
    Pause,
    Insert,
    NumLock,
    BackQuote,
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    BackSlash,
    SemiColon,
    Quote,
    Comma,
    Dot,
    Slash,
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    Unknown(String), // For keys that don't have a specific variant
}

impl From<Key> for SerializableKey {
    fn from(key: Key) -> Self {
        match key {
            Key::Alt => SerializableKey::Alt,
            Key::AltGr => SerializableKey::AltGr,
            Key::Backspace => SerializableKey::Backspace,
            Key::CapsLock => SerializableKey::CapsLock,
            Key::ControlLeft => SerializableKey::ControlLeft,
            Key::ControlRight => SerializableKey::ControlRight,
            Key::Delete => SerializableKey::Delete,
            Key::DownArrow => SerializableKey::DownArrow,
            Key::End => SerializableKey::End,
            Key::Escape => SerializableKey::Escape,
            Key::F1 => SerializableKey::F1,
            Key::F2 => SerializableKey::F2,
            Key::F3 => SerializableKey::F3,
            Key::F4 => SerializableKey::F4,
            Key::F5 => SerializableKey::F5,
            Key::F6 => SerializableKey::F6,
            Key::F7 => SerializableKey::F7,
            Key::F8 => SerializableKey::F8,
            Key::F9 => SerializableKey::F9,
            Key::F10 => SerializableKey::F10,
            Key::F11 => SerializableKey::F11,
            Key::F12 => SerializableKey::F12,
            Key::Home => SerializableKey::Home,
            Key::LeftArrow => SerializableKey::LeftArrow,
            Key::MetaLeft => SerializableKey::MetaLeft,
            Key::MetaRight => SerializableKey::MetaRight,
            Key::PageDown => SerializableKey::PageDown,
            Key::PageUp => SerializableKey::PageUp,
            Key::Return => SerializableKey::Return,
            Key::RightArrow => SerializableKey::RightArrow,
            Key::ShiftLeft => SerializableKey::ShiftLeft,
            Key::ShiftRight => SerializableKey::ShiftRight,
            Key::Space => SerializableKey::Space,
            Key::Tab => SerializableKey::Tab,
            Key::UpArrow => SerializableKey::UpArrow,
            Key::PrintScreen => SerializableKey::PrintScreen,
            Key::ScrollLock => SerializableKey::ScrollLock,
            Key::Pause => SerializableKey::Pause,
            Key::Insert => SerializableKey::Insert,
            Key::NumLock => SerializableKey::NumLock,
            Key::BackQuote => SerializableKey::BackQuote,
            Key::Minus => SerializableKey::Minus,
            Key::Equal => SerializableKey::Equal,
            Key::LeftBracket => SerializableKey::LeftBracket,
            Key::RightBracket => SerializableKey::RightBracket,
            Key::BackSlash => SerializableKey::BackSlash,
            Key::SemiColon => SerializableKey::SemiColon,
            Key::Quote => SerializableKey::Quote,
            Key::Comma => SerializableKey::Comma,
            Key::Dot => SerializableKey::Dot,
            Key::Slash => SerializableKey::Slash,
            Key::KeyA => SerializableKey::KeyA,
            Key::KeyB => SerializableKey::KeyB,
            Key::KeyC => SerializableKey::KeyC,
            Key::KeyD => SerializableKey::KeyD,
            Key::KeyE => SerializableKey::KeyE,
            Key::KeyF => SerializableKey::KeyF,
            Key::KeyG => SerializableKey::KeyG,
            Key::KeyH => SerializableKey::KeyH,
            Key::KeyI => SerializableKey::KeyI,
            Key::KeyJ => SerializableKey::KeyJ,
            Key::KeyK => SerializableKey::KeyK,
            Key::KeyL => SerializableKey::KeyL,
            Key::KeyM => SerializableKey::KeyM,
            Key::KeyN => SerializableKey::KeyN,
            Key::KeyO => SerializableKey::KeyO,
            Key::KeyP => SerializableKey::KeyP,
            Key::KeyQ => SerializableKey::KeyQ,
            Key::KeyR => SerializableKey::KeyR,
            Key::KeyS => SerializableKey::KeyS,
            Key::KeyT => SerializableKey::KeyT,
            Key::KeyU => SerializableKey::KeyU,
            Key::KeyV => SerializableKey::KeyV,
            Key::KeyW => SerializableKey::KeyW,
            Key::KeyX => SerializableKey::KeyX,
            Key::KeyY => SerializableKey::KeyY,
            Key::KeyZ => SerializableKey::KeyZ,
            Key::Num0 => SerializableKey::Num0,
            Key::Num1 => SerializableKey::Num1,
            Key::Num2 => SerializableKey::Num2,
            Key::Num3 => SerializableKey::Num3,
            Key::Num4 => SerializableKey::Num4,
            Key::Num5 => SerializableKey::Num5,
            Key::Num6 => SerializableKey::Num6,
            Key::Num7 => SerializableKey::Num7,
            Key::Num8 => SerializableKey::Num8,
            Key::Num9 => SerializableKey::Num9,
            _ => SerializableKey::Unknown(format!("{:?}", key)),
        }
    }
}

impl From<SerializableKey> for Key {
    fn from(key: SerializableKey) -> Self {
        match key {
            SerializableKey::Alt => Key::Alt,
            SerializableKey::AltGr => Key::AltGr,
            SerializableKey::Backspace => Key::Backspace,
            SerializableKey::CapsLock => Key::CapsLock,
            SerializableKey::ControlLeft => Key::ControlLeft,
            SerializableKey::ControlRight => Key::ControlRight,
            SerializableKey::Delete => Key::Delete,
            SerializableKey::DownArrow => Key::DownArrow,
            SerializableKey::End => Key::End,
            SerializableKey::Escape => Key::Escape,
            SerializableKey::F1 => Key::F1,
            SerializableKey::F2 => Key::F2,
            SerializableKey::F3 => Key::F3,
            SerializableKey::F4 => Key::F4,
            SerializableKey::F5 => Key::F5,
            SerializableKey::F6 => Key::F6,
            SerializableKey::F7 => Key::F7,
            SerializableKey::F8 => Key::F8,
            SerializableKey::F9 => Key::F9,
            SerializableKey::F10 => Key::F10,
            SerializableKey::F11 => Key::F11,
            SerializableKey::F12 => Key::F12,
            SerializableKey::Home => Key::Home,
            SerializableKey::LeftArrow => Key::LeftArrow,
            SerializableKey::MetaLeft => Key::MetaLeft,
            SerializableKey::MetaRight => Key::MetaRight,
            SerializableKey::PageDown => Key::PageDown,
            SerializableKey::PageUp => Key::PageUp,
            SerializableKey::Return => Key::Return,
            SerializableKey::RightArrow => Key::RightArrow,
            SerializableKey::ShiftLeft => Key::ShiftLeft,
            SerializableKey::ShiftRight => Key::ShiftRight,
            SerializableKey::Space => Key::Space,
            SerializableKey::Tab => Key::Tab,
            SerializableKey::UpArrow => Key::UpArrow,
            SerializableKey::PrintScreen => Key::PrintScreen,
            SerializableKey::ScrollLock => Key::ScrollLock,
            SerializableKey::Pause => Key::Pause,
            SerializableKey::Insert => Key::Insert,
            SerializableKey::NumLock => Key::NumLock,
            SerializableKey::BackQuote => Key::BackQuote,
            SerializableKey::Minus => Key::Minus,
            SerializableKey::Equal => Key::Equal,
            SerializableKey::LeftBracket => Key::LeftBracket,
            SerializableKey::RightBracket => Key::RightBracket,
            SerializableKey::BackSlash => Key::BackSlash,
            SerializableKey::SemiColon => Key::SemiColon,
            SerializableKey::Quote => Key::Quote,
            SerializableKey::Comma => Key::Comma,
            SerializableKey::Dot => Key::Dot,
            SerializableKey::Slash => Key::Slash,
            SerializableKey::KeyA => Key::KeyA,
            SerializableKey::KeyB => Key::KeyB,
            SerializableKey::KeyC => Key::KeyC,
            SerializableKey::KeyD => Key::KeyD,
            SerializableKey::KeyE => Key::KeyE,
            SerializableKey::KeyF => Key::KeyF,
            SerializableKey::KeyG => Key::KeyG,
            SerializableKey::KeyH => Key::KeyH,
            SerializableKey::KeyI => Key::KeyI,
            SerializableKey::KeyJ => Key::KeyJ,
            SerializableKey::KeyK => Key::KeyK,
            SerializableKey::KeyL => Key::KeyL,
            SerializableKey::KeyM => Key::KeyM,
            SerializableKey::KeyN => Key::KeyN,
            SerializableKey::KeyO => Key::KeyO,
            SerializableKey::KeyP => Key::KeyP,
            SerializableKey::KeyQ => Key::KeyQ,
            SerializableKey::KeyR => Key::KeyR,
            SerializableKey::KeyS => Key::KeyS,
            SerializableKey::KeyT => Key::KeyT,
            SerializableKey::KeyU => Key::KeyU,
            SerializableKey::KeyV => Key::KeyV,
            SerializableKey::KeyW => Key::KeyW,
            SerializableKey::KeyX => Key::KeyX,
            SerializableKey::KeyY => Key::KeyY,
            SerializableKey::KeyZ => Key::KeyZ,
            SerializableKey::Num0 => Key::Num0,
            SerializableKey::Num1 => Key::Num1,
            SerializableKey::Num2 => Key::Num2,
            SerializableKey::Num3 => Key::Num3,
            SerializableKey::Num4 => Key::Num4,
            SerializableKey::Num5 => Key::Num5,
            SerializableKey::Num6 => Key::Num6,
            SerializableKey::Num7 => Key::Num7,
            SerializableKey::Num8 => Key::Num8,
            SerializableKey::Num9 => Key::Num9,
            SerializableKey::Unknown(_) => Key::Unknown(0), // Default fallback
        }
    }
}

impl From<rdev::Button> for MouseButton {
    fn from(button: rdev::Button) -> Self {
        match button {
            rdev::Button::Left => MouseButton::Left,
            rdev::Button::Right => MouseButton::Right,
            rdev::Button::Middle => MouseButton::Middle,
            // For other buttons, we'll use a simpler approach since direct casting doesn't work
            _ => MouseButton::Unknown(0), // Default to 0 for unknown buttons
        }
    }
}

impl From<MouseButton> for rdev::Button {
    fn from(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => rdev::Button::Left,
            MouseButton::Right => rdev::Button::Right,
            MouseButton::Middle => rdev::Button::Middle,
            MouseButton::Unknown(code) => unsafe { std::mem::transmute(code) }, // Be careful with this
        }
    }
}

/// Message types for communication between master and slave clients
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageType {
    /// Authentication message with shared key
    Authenticate { key: String },
    /// Authentication response
    AuthResponse { success: bool },
    /// Keyboard event data
    Keyboard { event: KeyboardEvent },
    /// Mouse event data
    Mouse { event: MouseEvent },
    /// Encrypted data payload
    Data { payload: Vec<u8> },
    /// Heartbeat to maintain connection
    Heartbeat,
    /// Disconnect notification
    Disconnect,
}

/// Main client implementation
pub struct LanDataClient {}

impl LanDataClient {
    /// Create a new client
    pub fn new() -> Result<Self> {
        Ok(LanDataClient {})
    }

    /// Start the client based on its role
    pub async fn start(&self, config_path: Option<&str>, port: Option<u16>, key: Option<&str>) -> Result<()> {
        match (config_path, port, key) {
            (Some(config_path), None, None) => {
                // Master client: expects config file path
                let master = MasterClient::new()?;
                master.start(config_path).await
            },
            (None, Some(port), Some(key)) => {
                // Slave client: expects port and key
                let slave = SlaveClient::new()?;
                slave.start(port, key).await
            },
            _ => {
                Err(anyhow::anyhow!("Invalid arguments: provide either config file path for master or port and key for slave"))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Example usage:
    // Parse command line arguments to determine role and settings
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  For master: {} master <config_path>", args[0]);
        eprintln!("  For slave: {} slave <port> <shared_key>", args[0]);
        eprintln!("Example: {} master ./config.json", args[0]);
        eprintln!("Example: {} slave 8081 mysecretkey", args[0]);
        std::process::exit(1);
    }
    
    let role_str = &args[1];
    let role = match role_str.as_str() {
        "master" => ClientRole::Master,
        "slave" => ClientRole::Slave,
        _ => {
            eprintln!("Invalid role: {}. Use 'master' or 'slave'", role_str);
            std::process::exit(1);
        }
    };
    
    let client = LanDataClient::new()?;
    
    match role {
        ClientRole::Master => {
            // Master client: expects config file path as second argument
            if args.len() < 3 {
                eprintln!("Master client requires config file path as second argument");
                eprintln!("Usage: {} master <config_path>", args[0]);
                std::process::exit(1);
            }
            
            let config_path = &args[2];
            client.start(Some(config_path), None, None).await?;
        },
        ClientRole::Slave => {
            // Slave client: expects port as second argument and shared key as third argument
            if args.len() < 4 {
                eprintln!("Slave client requires port and shared key as arguments");
                eprintln!("Usage: {} slave <port> <shared_key>", args[0]);
                std::process::exit(1);
            }
            
            let port = args[2].parse::<u16>().context("Invalid port number")?;
            let key = &args[3]; // Use shared key directly from command line argument
            
            client.start(None, Some(port), Some(key)).await?;
        }
    }
    
    Ok(())
}

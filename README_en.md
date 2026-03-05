# CrossInput - Cross-Machine Input Sharing

[中文版 (Chinese Version)](README_cn.md)

CrossInput is a Rust-based application that enables sharing keyboard and mouse inputs across machines in a local network. It forwards keyboard and mouse inputs from the current host to other computers via LAN connection. Currently, only Windows is supported.

## Features

- **Cross-Machine Input Control**: Share keyboard and mouse inputs from one machine to others in the local network
- **Master-Slave Architecture**: Supports master and slave client roles
- **Secure Authentication**: Uses shared keys for authentication
- **Encrypted Transmission**: All data is encrypted with AES-GCM
- **Low Latency**: Data transmission delay under 100ms
- **Persistent Connection**: Maintains stable long-term connections

## Architecture

- **Master Client**: Listens for connections from slave clients, responsible for capturing and sending input data
- **Slave Client**: Connects to the master client, receives and processes input data
- **Authentication Mechanism**: Uses shared keys to verify client identity
- **Encrypted Communication**: All data transmission is encrypted

## Requirements

- Rust 1.70 or higher
- Cargo package manager

## Usage

### 1. Create Configuration File (Optional)

Create a `config.json` file with the following content:

```json
{
  "ip": "0.0.0.0",
  "port": 8080,
  "slave_connections": [
    {
      "address": "192.168.1.1:8081",
      "key": "aaa"
    }
  ]
}
```

### 2. Start Master Client

```bash
cargo run -- master <config_path>
```

Example:
```bash
cargo run -- master ./config.json
```

### 3. Start Slave Client

```bash
cargo run -- slave <port> <shared_key>
```

Example:
```bash
cargo run -- slave 8081 mysecretkey
```

## Example Scenario

1. Start the master client on one machine:
   ```bash
   cargo run -- master ./config.json
   ```

2. Start the slave client on another machine (or different terminal on the same machine):
   ```bash
   cargo run -- slave 8081 mysecretkey
   ```

3. The master client will forward keyboard and mouse inputs to the slave client, allowing control of the slave machine

## Technical Details

- Uses Tokio async runtime
- TCP-based reliable connection
- JSON-formatted message protocol
- AES-256-GCM encryption algorithm
- Automatic reconnection mechanism (slave client)

## Message Protocol

Message types include:
- `Authenticate`: Authentication message
- `AuthResponse`: Authentication response
- `Data`: Data payload
- `Heartbeat`: Heartbeat packet
- `Disconnect`: Disconnect notification

## Error Handling

- Connection timeout handling
- Authentication failure handling
- Network exception handling
- Data decryption failure handling

## Performance Optimization

- Low-latency data transmission (< 100ms)
- Efficient async I/O operations
- Memory-friendly data processing

## Windows Specific Notes

To capture all keyboard events (including system shortcuts like Alt+Tab), the master client must be run with administrator privileges on Windows systems. This is due to Windows security restrictions on low-level keyboard hooks, which require administrator permissions to intercept system-level shortcuts.

## Hotkey

- **Right Shift**: Pause/Resume input forwarding
  - Press once: Pause input forwarding
  - Press again: Resume input forwarding

## License

This project is licensed under the GPL-3.0 License - see the [LICENSE](LICENSE) file for details.

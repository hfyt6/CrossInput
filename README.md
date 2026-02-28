# 局域网数据传输客户端

这是一个用 Rust 编写的局域网内数据传输客户端，支持主从架构的数据传输。

## 功能特性

- **主从架构**: 支持主客户端和从客户端两种角色
- **安全认证**: 使用共享密钥进行身份验证
- **加密传输**: 所有数据均经过 AES-GCM 加密
- **低延迟**: 数据传输延迟低于 100ms
- **长连接**: 保持长时间稳定连接

## 架构说明

- **主客户端**: 监听来自从客户端的连接，负责发送数据
- **从客户端**: 连接到主客户端，负责接收数据
- **认证机制**: 使用共享密钥验证客户端身份
- **加密通信**: 所有数据传输都经过加密处理

## 安装要求

- Rust 1.70 或更高版本
- Cargo 包管理器

## 使用方法

### 1. 创建配置文件（可选）

创建一个 `config.json` 文件，内容如下：

```json
{
  "shared_key": "mysecretkey",
  "ip": "0.0.0.0",
  "port": 8080,
  "slave_addresses": []
}
```

### 2. 启动主客户端

```bash
cargo run -- master <config_path>
```

例如：
```bash
cargo run -- master ./config.json
```

### 3. 启动从客户端

```bash
cargo run -- slave <port> <shared_key>
```

例如：
```bash
cargo run -- slave 8081 mysecretkey
```

## 示例场景

1. 在一台机器上启动主客户端：
   ```bash
   cargo run -- master ./config.json
   ```

2. 在另一台机器上启动从客户端（或同一台机器的不同终端）：
   ```bash
   cargo run -- slave 8081 mysecretkey
   ```

3. 主客户端将开始向从客户端发送数据，从客户端会接收并显示这些数据

## 技术细节

- 使用 Tokio 异步运行时
- 基于 TCP 的可靠连接
- JSON 格式的消息协议
- AES-256-GCM 加密算法
- 自动重连机制（从客户端）

## 协议格式

消息类型包括：
- `Authenticate`: 认证消息
- `AuthResponse`: 认证响应
- `Data`: 数据载荷
- `Heartbeat`: 心跳包
- `Disconnect`: 断开连接通知

## 错误处理

- 连接超时处理
- 认证失败处理
- 网络异常处理
- 数据解密失败处理

## 性能优化

- 低延迟数据传输（< 100ms）
- 高效的异步 I/O 操作
- 内存友好的数据处理
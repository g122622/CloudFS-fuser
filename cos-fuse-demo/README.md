# COS FUSE Demo

一个基于 `fuser` 的腾讯云 COS 对象存储文件系统，将 COS bucket 挂载为本地 POSIX 文件系统。

## 功能特性

- ✅ 将 COS bucket 挂载为本地目录
- ✅ 支持 `readdir`（列出目录）
- ✅ 支持 `getattr`（获取文件属性）
- ✅ 支持 `open` + `read`（读取文件）
- ✅ L1 缓存：内存缓存文件元数据（避免重复 HEAD 请求）
- ✅ L2 缓存：本地 SSD 缓存已读取的文件内容（避免重复 GET）
- ❌ 写入、删除等复杂操作（Demo 版本只读）

## 系统要求

- macOS (已在 M1 MacBook 上测试)
- Rust 1.70+
- FUSE for macOS (macFUSE)

## 安装依赖

### 1. 安装 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 2. 安装 macFUSE

```bash
brew install --cask macfuse
```

## 构建项目

```bash
cd cos-fuse-demo
cargo build --release
```

## 使用方法

### 基本用法

```bash
# 创建挂载点
mkdir -p /mnt/cosfs

# 挂载 COS bucket（假设为公开读 bucket）
./target/release/cos-fuse-demo \
  --bucket your-bucket-name \
  --region ap-beijing \
  --mount-point /mnt/cosfs \
  --cache-dir /tmp/cosfs_cache \
  --foreground
```

### 命令行参数

- `--bucket, -b`: COS bucket 名称（必需）
- `--region, -r`: COS 区域（必需，如 ap-beijing）
- `--mount-point, -m`: 挂载点目录（必需）
- `--cache-dir, -c`: 缓存目录（默认：/tmp/cosfs_cache）
- `--foreground, -f`: 前台运行
- `--debug, -d`: 启用调试日志

## 测试验证

```bash
# 列出根目录
ls /mnt/cosfs

# 列出子目录
ls /mnt/cosfs/data

# 读取文件
cat /mnt/cosfs/data/file1.txt

# 查看文件属性
stat /mnt/cosfs/data/file1.txt
```

## 缓存验证

第一次读取文件时会从 COS 下载，后续读取会直接从本地缓存：

```bash
# 第一次读取（会触发网络请求）
time cat /mnt/cosfs/data/file1.txt

# 第二次读取（从缓存读取，速度更快）
time cat /mnt/cosfs/data/file1.txt
```

## 卸载文件系统

```bash
# 如果使用前台运行，按 Ctrl+C 自动卸载
# 或者手动卸载
umount /mnt/cosfs
```

## 项目结构

```
cos-fuse-demo/
├── Cargo.toml              # 项目配置和依赖
├── src/
│   ├── main.rs             # 主程序入口
│   ├── filesystem.rs       # FUSE 文件系统实现
│   ├── cos_client.rs       # 腾讯云 COS 客户端
│   └── cache.rs            # L1/L2 缓存实现
└── README.md               # 项目说明
```

## 技术实现

### 虚拟目录结构

由于 COS 是扁平的键值存储，没有真实的目录结构，本系统通过以下方式模拟目录：

1. 解析对象键中的 `/` 分隔符
2. 为每个路径层级创建虚拟目录
3. 在 `readdir` 时动态构建目录内容

### 缓存策略

- **L1 元数据缓存**：使用 `lru::LruCache` 在内存中缓存文件元数据
- **L2 内容缓存**：将文件内容缓存到本地文件系统

### inode 管理

- 根目录 inode = 1
- 动态分配 inode >= 1000
- 维护 inode ↔ 路径的双向映射

## 注意事项

1. **只读模式**：当前版本只支持读取，不支持写入和删除操作
2. **公开 bucket**：Demo 版本假设 bucket 为公开读，无需认证
3. **性能**：Demo 版本重点在功能实现，性能优化有限
4. **错误处理**：网络错误可能导致文件系统响应变慢

## 扩展建议

1. **认证支持**：集成腾讯云 COS SDK，支持私有 bucket
2. **异步优化**：使用 tokio 异步处理，提升并发性能
3. **预取机制**：启动时预加载对象列表和元数据
4. **写入支持**：实现文件上传和删除功能
5. **性能优化**：批量操作、连接池等

## 故障排除

### 挂载失败

- 检查挂载点目录是否存在且有权限
- 确认 macFUSE 已正确安装
- 检查日志输出中的错误信息

### 文件读取失败

- 确认 bucket 名称和区域正确
- 检查网络连接
- 验证对象键是否存在

### 性能问题

- 检查缓存目录的磁盘空间
- 调整缓存大小配置
- 启用调试日志查看详细操作信息

## 许可证

MIT License

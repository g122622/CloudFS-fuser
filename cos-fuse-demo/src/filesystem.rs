use anyhow::{anyhow, Result};
use fuser::{
    FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyXattr, Request,
};
use libc::{EACCES, EIO, ENODATA, ENOENT, ENOTDIR, EPERM};
use log::{debug, error, info, warn};
use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::runtime::Runtime;

use crate::cache::Cache;
use crate::cos_client::{CosClient, ObjectMeta};

/// 文件系统 inode 分配器
const ROOT_INODE: u64 = 1;
const FIRST_DYNAMIC_INODE: u64 = 2;

/// 目录条目
#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    ino: u64,
    file_type: FileType,
}

/// COS 文件系统实现
pub struct CosFilesystem {
    /// COS 客户端
    cos_client: CosClient,

    /// 缓存系统
    cache: Cache,

    /// inode 到路径的映射
    inode_to_path: HashMap<u64, String>,

    /// 路径到 inode 的映射
    path_to_inode: HashMap<String, u64>,

    /// 下一个可用的 inode 号
    next_inode: u64,

    /// 对象列表缓存（用于构建虚拟目录结构）
    object_list: Vec<String>,

    /// 目录条目缓存（减少重复的readdir调用）
    dir_cache: HashMap<String, Vec<DirEntry>>,

    /// 共享的异步运行时
    runtime: Arc<Runtime>,
}

impl CosFilesystem {
    pub fn new(bucket: String, region: String, cache_dir: &Path) -> Result<Self> {
        let cos_client = CosClient::new(bucket, region);
        let cache = Cache::new(cache_dir, 1000)?;

        // 创建共享的运行时
        let runtime = Runtime::new().map_err(|e| anyhow!("Failed to create runtime: {}", e))?;

        let mut fs = Self {
            cos_client,
            cache,
            inode_to_path: HashMap::new(),
            path_to_inode: HashMap::new(),
            next_inode: FIRST_DYNAMIC_INODE,
            object_list: Vec::new(),
            dir_cache: HashMap::new(),
            runtime: Arc::new(runtime),
        };

        // 初始化根目录
        fs.inode_to_path.insert(ROOT_INODE, "/".to_string());
        fs.path_to_inode.insert("/".to_string(), ROOT_INODE);

        Ok(fs)
    }

    /// 分配新的 inode
    fn allocate_inode(&mut self) -> u64 {
        let ino = self.next_inode;
        self.next_inode += 1;
        ino
    }

    /// 获取路径对应的 inode，如果不存在则创建
    fn get_or_create_inode(&mut self, path: &str) -> u64 {
        if let Some(&ino) = self.path_to_inode.get(path) {
            return ino;
        }

        let ino = self.allocate_inode();
        self.inode_to_path.insert(ino, path.to_string());
        self.path_to_inode.insert(path.to_string(), ino);
        ino
    }

    /// 获取 inode 对应的路径
    fn get_path(&self, ino: u64) -> Option<&String> {
        // 捕获调用栈用于调试
        let backtrace = Backtrace::force_capture();
        info!(
            "get_path called with ino: {}, backtrace:\n{}",
            ino, backtrace
        );

        self.inode_to_path.get(&ino)
    }

    /// 刷新对象列表（非借用版本）
    async fn refresh_object_list_async(&mut self) -> Result<()> {
        info!("Refreshing object list from COS");
        self.object_list = self.cos_client.list_objects().await?;

        // 清理旧的 inode 映射（保留根目录）
        self.inode_to_path.clear();
        self.path_to_inode.clear();
        self.next_inode = FIRST_DYNAMIC_INODE;

        // 清空目录缓存
        self.dir_cache.clear();

        // 重新添加根目录
        self.inode_to_path.insert(ROOT_INODE, "/".to_string());
        self.path_to_inode.insert("/".to_string(), ROOT_INODE);

        // 为所有对象路径创建 inode 映射
        for object_key in self.object_list.clone() {
            let path = format!("/{}", object_key);
            self.get_or_create_inode(&path);

            // 为所有父目录创建 inode
            let mut current_path = Path::new(&path).parent().unwrap_or(Path::new("/"));
            while current_path != Path::new("/") {
                let current_path_str = current_path.to_string_lossy();
                self.get_or_create_inode(&current_path_str);
                current_path = current_path.parent().unwrap_or(Path::new("/"));
            }
        }

        info!("Loaded {} objects from COS", self.object_list.len());
        Ok(())
    }

    /// 刷新对象列表
    async fn refresh_object_list(&mut self) -> Result<()> {
        self.refresh_object_list_async().await
    }

    /// 获取对象的元数据
    async fn get_object_metadata(&self, key: &str) -> Result<ObjectMeta> {
        // 先检查缓存
        if let Some(meta) = self.cache.get_metadata(key) {
            debug!("Metadata cache hit for key: {}", key);
            return Ok(meta);
        }

        debug!("Metadata cache miss for key: {}, fetching from COS", key);
        let meta = self.cos_client.head_object(key).await?;

        // 缓存元数据
        self.cache.set_metadata(key.to_string(), meta.clone());

        Ok(meta)
    }

    /// 获取对象的元数据并转换为FileAttr
    async fn get_object_meta(&mut self, path: &str) -> Result<FileAttr> {
        // 先检查缓存
        if let Some(meta) = self.cache.get_metadata(path) {
            let ino = self.get_or_create_inode(path);
            return Ok(self.meta_to_attr(&meta, ino));
        }

        // 从 COS 获取元数据
        let meta = self.cos_client.head_object(&path[1..]).await?; // 去掉开头的 '/'

        // 缓存元数据
        self.cache.set_metadata(path.to_string(), meta.clone());

        let ino = self.get_or_create_inode(path);
        Ok(self.meta_to_attr(&meta, ino))
    }

    /// 获取对象内容
    async fn get_object_content(&self, key: &str) -> Result<Vec<u8>> {
        // 先检查 L2 缓存
        if self.cache.is_content_cached(key) {
            debug!("Content cache hit for key: {}", key);
            return self.cache.get_cached_content(key);
        }

        debug!("Content cache miss for key: {}, downloading from COS", key);
        let content = self.cos_client.get_object(key).await?;

        // 缓存内容
        self.cache.cache_content(key, &content)?;

        Ok(content.to_vec())
    }

    /// 将 ObjectMeta 转换为 FileAttr
    fn meta_to_attr(&self, meta: &ObjectMeta, ino: u64) -> FileAttr {
        FileAttr {
            ino,
            size: meta.size,
            blocks: (meta.size + 511) / 512, // 块大小为 512 字节
            atime: meta.last_modified,
            mtime: meta.last_modified,
            ctime: meta.last_modified,
            crtime: meta.last_modified,
            kind: FileType::RegularFile,
            perm: 0o644, // 默认文件权限
            nlink: 1,
            uid: 501, // 默认用户 ID
            gid: 20,  // 默认组 ID
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// 创建目录属性
    fn create_dir_attr(&self, ino: u64) -> FileAttr {
        let now = SystemTime::now();
        FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Directory,
            perm: 0o755, // 默认目录权限
            nlink: 2,
            uid: 501,
            gid: 20,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// 判断路径是否是目录
    fn is_directory(&self, path: &str) -> bool {
        if path == "/" {
            return true;
        }

        // 检查是否有任何对象以该路径为前缀（后面跟着'/'）
        let path_with_slash = if path == "/" {
            "/".to_string()
        } else {
            format!("{}/", path.trim_start_matches('/'))
        };

        self.object_list.iter().any(|obj| {
            let obj_path = format!("/{}", obj);
            obj_path.starts_with(&path_with_slash) && obj_path != path
        })
    }

    /// 列出目录内容
    fn list_directory(&self, path: &str) -> Vec<DirEntry> {
        let mut entries = Vec::new();
        let path_prefix = path.trim_start_matches('/');

        if path == "/" {
            // 根目录，列出第一级目录和文件
            let mut seen_names = std::collections::HashSet::new();

            for object_key in &self.object_list {
                let parts: Vec<&str> = object_key.split('/').collect();
                if parts.len() >= 1 {
                    let name = parts[0];
                    if !seen_names.contains(name) {
                        seen_names.insert(name);

                        let full_path = format!("/{}", name);
                        let ino = *self.path_to_inode.get(&full_path).unwrap();

                        if parts.len() > 1 {
                            // 这是一个目录
                            entries.push(DirEntry {
                                name: name.to_string(),
                                ino,
                                file_type: FileType::Directory,
                            });
                        } else {
                            // 这是一个文件
                            entries.push(DirEntry {
                                name: name.to_string(),
                                ino,
                                file_type: FileType::RegularFile,
                            });
                        }
                    }
                }
            }
        } else {
            // 子目录
            let mut seen_names = std::collections::HashSet::new();

            for object_key in &self.object_list {
                if object_key.starts_with(path_prefix) {
                    let relative_path = &object_key[path_prefix.len()..];
                    let relative_path = relative_path.trim_start_matches('/');

                    if let Some(slash_pos) = relative_path.find('/') {
                        // 这是一个子目录
                        let dir_name = &relative_path[..slash_pos];
                        if !seen_names.contains(dir_name) {
                            seen_names.insert(dir_name);

                            let full_path = format!("{}/{}", path, dir_name);
                            let ino = *self.path_to_inode.get(&full_path).unwrap();

                            entries.push(DirEntry {
                                name: dir_name.to_string(),
                                ino,
                                file_type: FileType::Directory,
                            });
                        }
                    } else if !relative_path.is_empty() {
                        // 这是一个文件
                        let full_path = format!("/{}", object_key);
                        let ino = *self.path_to_inode.get(&full_path).unwrap();

                        entries.push(DirEntry {
                            name: relative_path.to_string(),
                            ino,
                            file_type: FileType::RegularFile,
                        });
                    }
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }
}

impl Filesystem for CosFilesystem {
    fn init(&mut self, _req: &Request<'_>, _config: &mut KernelConfig) -> Result<(), i32> {
        info!("Initializing COS filesystem");

        // 在初始化时刷新对象列表
        let rt = Arc::clone(&self.runtime);

        if let Err(e) = rt.block_on(self.refresh_object_list_async()) {
            error!("Failed to initialize object list: {}", e);
            return Err(EIO);
        }

        info!("COS filesystem initialized successfully");
        Ok(())
    }

    fn destroy(&mut self) {
        info!("Destroying COS filesystem");

        // 清理缓存
        if let Err(e) = self.cache.clear() {
            warn!("Failed to clear cache: {}", e);
        }

        info!("COS filesystem destroyed");
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        info!("Lookup: parent={}, name={}", parent, name.display());

        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let parent_path = match self.get_path(parent) {
            Some(p) => p.clone(),
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let target_path = if parent_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_path, name_str)
        };

        debug!(
            "Lookup: parent={}, name={}, target_path={}",
            parent, name_str, target_path
        );

        // 检查是否是目录
        if self.is_directory(&target_path) {
            let ino = self.get_or_create_inode(&target_path);
            let attr = self.create_dir_attr(ino);
            reply.entry(&Duration::from_secs(1), &attr, 0);
            return;
        }

        // 检查是否是文件
        let object_key = target_path.trim_start_matches('/');
        if self.object_list.contains(&object_key.to_string()) {
            let ino = self.get_or_create_inode(&target_path);

            let rt = Arc::clone(&self.runtime);

            match rt.block_on(self.get_object_metadata(object_key)) {
                Ok(meta) => {
                    let attr = self.meta_to_attr(&meta, ino);
                    reply.entry(&Duration::from_secs(1), &attr, 0);
                }
                Err(e) => {
                    error!("Failed to get metadata for {}: {}", object_key, e);
                    reply.error(EIO);
                }
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        info!("Getattr: ino={}", ino);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        debug!("Getattr: ino={}, path={}", ino, path);

        if self.is_directory(path) {
            let attr = self.create_dir_attr(ino);
            reply.attr(&Duration::from_secs(1), &attr);
        } else {
            let object_key = path.trim_start_matches('/');

            let rt = Arc::clone(&self.runtime);

            match rt.block_on(self.get_object_metadata(object_key)) {
                Ok(meta) => {
                    let attr = self.meta_to_attr(&meta, ino);
                    reply.attr(&Duration::from_secs(1), &attr);
                }
                Err(e) => {
                    error!("Failed to get metadata for {}: {}", object_key, e);
                    reply.error(EIO);
                }
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let path = match self.get_path(ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        if !self.is_directory(&path) {
            reply.error(ENOTDIR);
            return;
        }

        // --- 修复点：避免在 or_insert_with 中捕获 self ---
        let entries = if let Some(cached) = self.dir_cache.get(&path) {
            cached.clone()
        } else {
            let listed = self.list_directory(&path);
            self.dir_cache.insert(path.clone(), listed.clone());
            listed
        };

        // 构建完整 entry 列表
        let mut all_entries = Vec::with_capacity(entries.len() + 2);

        // "."
        all_entries.push((ino, FileType::Directory, ".".to_string()));

        // ".."
        let parent_ino = if path == "/" {
            ino
        } else {
            let parent_path = Path::new(&path).parent().unwrap_or(Path::new("/"));
            let parent_path_str = parent_path.to_string_lossy().to_string();
            *self
                .path_to_inode
                .get(&parent_path_str)
                .unwrap_or(&ROOT_INODE)
        };
        all_entries.push((parent_ino, FileType::Directory, "..".to_string()));

        // 真实条目
        all_entries.extend(entries.into_iter().map(|e| (e.ino, e.file_type, e.name)));

        // 发送目录项
        for (index, (ino, kind, name)) in all_entries.into_iter().enumerate() {
            let next_offset = (index + 1) as i64;
            if (index as i64) >= offset {
                if reply.add(ino, next_offset, kind, &name) {
                    break; // buffer full
                }
            }
        }

        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        info!("Open: ino={}", ino);

        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        debug!("Open: ino={}, path={}", ino, path);

        // 只允许打开文件，不允许打开目录
        if self.is_directory(path) {
            reply.error(EPERM);
            return;
        }

        reply.opened(0, 0);
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let path = match self.get_path(ino) {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        debug!(
            "Read: ino={}, path={}, offset={}, size={}",
            ino, path, offset, size
        );

        if self.is_directory(path) {
            reply.error(EPERM);
            return;
        }

        let object_key = path.trim_start_matches('/');

        let rt = Arc::clone(&self.runtime);

        match rt.block_on(self.get_object_content(object_key)) {
            Ok(content) => {
                let start = offset as usize;
                let end = std::cmp::min(start + size as usize, content.len());

                if start >= content.len() {
                    reply.data(&[]);
                } else {
                    reply.data(&content[start..end]);
                }
            }
            Err(e) => {
                error!("Failed to read object {}: {}", object_key, e);
                reply.error(EIO);
            }
        }
    }

    fn access(&mut self, _req: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        debug!("Access: ino={}, mask={}", ino, mask);

        // 检查文件/目录是否存在
        if self.get_path(ino).is_none() {
            reply.error(ENOENT);
            return;
        }

        // 对于COS文件系统，我们假设所有文件都有读权限
        // 写权限暂时不支持，因为COS是只读的
        if mask & libc::W_OK != 0 {
            // 拒绝写权限
            reply.error(EACCES);
        } else {
            // 允许读和执行权限
            reply.ok();
        }
    }

    fn listxattr(&mut self, _req: &Request<'_>, _ino: u64, size: u32, reply: ReplyXattr) {
        // 不支持扩展属性：返回空列表
        if size == 0 {
            reply.size(0); // 只需返回所需 buffer 大小（0）
        } else {
            reply.data(&[]); // 实际返回空数据
        }
    }

    fn getxattr(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _name: &std::ffi::OsStr,
        size: u32,
        reply: ReplyXattr,
    ) {
        // 不支持任何扩展属性
        if size == 0 {
            // 应用程序只查询值的大小（通常用于分配 buffer）
            // 因为属性不存在，返回 0 或错误均可，但标准做法是返回错误
            reply.error(ENODATA);
        } else {
            // 尝试读取不存在的属性
            reply.error(ENODATA);
        }
    }
}

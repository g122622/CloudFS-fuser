use anyhow::{anyhow, Result};
use lru::LruCache;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::cos_client::ObjectMeta;

pub struct Cache {
    /// L1 缓存：内存中的元数据缓存
    metadata_cache: Mutex<LruCache<String, ObjectMeta>>,
    
    /// L2 缓存：本地文件内容缓存
    cache_dir: PathBuf,
}

impl Cache {
    pub fn new(cache_dir: &Path, metadata_cache_size: usize) -> Result<Self> {
        // 创建缓存目录
        fs::create_dir_all(cache_dir)?;
        
        Ok(Self {
            metadata_cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(metadata_cache_size)
                    .ok_or_else(|| anyhow!("Invalid cache size"))?,
            )),
            cache_dir: cache_dir.to_path_buf(),
        })
    }

    /// 获取元数据缓存
    pub fn get_metadata(&self, key: &str) -> Option<ObjectMeta> {
        let mut cache = self.metadata_cache.lock().unwrap();
        cache.get(key).cloned()
    }

    /// 设置元数据缓存
    pub fn set_metadata(&self, key: String, meta: ObjectMeta) {
        let mut cache = self.metadata_cache.lock().unwrap();
        cache.put(key, meta);
    }

    /// 获取文件内容缓存路径
    pub fn get_content_cache_path(&self, key: &str) -> PathBuf {
        // 使用 URL 安全的文件名
        let safe_key = key.replace('/', "_").replace('\\', "_");
        self.cache_dir.join(format!("{}.cache", safe_key))
    }

    /// 检查文件内容是否已缓存
    pub fn is_content_cached(&self, key: &str) -> bool {
        let cache_path = self.get_content_cache_path(key);
        cache_path.exists()
    }

    /// 获取缓存的内容
    pub fn get_cached_content(&self, key: &str) -> Result<Vec<u8>> {
        let cache_path = self.get_content_cache_path(key);
        if !cache_path.exists() {
            return Err(anyhow!("Content not cached for key: {}", key));
        }
        
        fs::read(cache_path).map_err(|e| anyhow!("Failed to read cached content: {}", e))
    }

    /// 缓存文件内容
    pub fn cache_content(&self, key: &str, content: &[u8]) -> Result<()> {
        let cache_path = self.get_content_cache_path(key);
        
        // 确保父目录存在
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        fs::write(&cache_path, content)
            .map_err(|e| anyhow!("Failed to cache content: {}", e))?;
        
        Ok(())
    }

    /// 清理缓存
    pub fn clear(&self) -> Result<()> {
        // 清理元数据缓存
        {
            let mut cache = self.metadata_cache.lock().unwrap();
            cache.clear();
        }
        
        // 清理文件内容缓存
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)?;
            fs::create_dir_all(&self.cache_dir)?;
        }
        
        Ok(())
    }

    /// 获取缓存统计信息
    pub fn get_stats(&self) -> CacheStats {
        let metadata_cache_size = {
            let cache = self.metadata_cache.lock().unwrap();
            cache.len()
        };
        
        let content_cache_size = if self.cache_dir.exists() {
            fs::read_dir(&self.cache_dir)
                .map(|entries| entries.count())
                .unwrap_or(0)
        } else {
            0
        };
        
        CacheStats {
            metadata_cache_size,
            content_cache_size,
        }
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub metadata_cache_size: usize,
    pub content_cache_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_creation() {
        let temp_dir = TempDir::new().unwrap();
        let cache = Cache::new(temp_dir.path(), 100).unwrap();
        assert!(cache.cache_dir.exists());
    }

    #[test]
    fn test_metadata_cache() {
        let temp_dir = TempDir::new().unwrap();
        let cache = Cache::new(temp_dir.path(), 100).unwrap();
        
        let meta = ObjectMeta {
            key: "test.txt".to_string(),
            size: 100,
            last_modified: std::time::SystemTime::now(),
            etag: "test-etag".to_string(),
            content_type: Some("text/plain".to_string()),
        };
        
        // 测试设置和获取
        cache.set_metadata("test.txt".to_string(), meta.clone());
        let cached_meta = cache.get_metadata("test.txt");
        assert!(cached_meta.is_some());
        assert_eq!(cached_meta.unwrap().size, 100);
    }

    #[test]
    fn test_content_cache() {
        let temp_dir = TempDir::new().unwrap();
        let cache = Cache::new(temp_dir.path(), 100).unwrap();
        
        let key = "test/file.txt";
        let content = b"Hello, World!";
        
        // 测试缓存内容
        cache.cache_content(key, content).unwrap();
        assert!(cache.is_content_cached(key));
        
        // 测试获取缓存内容
        let cached_content = cache.get_cached_content(key).unwrap();
        assert_eq!(cached_content, content);
    }
}
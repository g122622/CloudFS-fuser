use anyhow::{anyhow, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub last_modified: SystemTime,
    pub etag: String,
    pub content_type: Option<String>,
}

#[derive(Debug)]
pub struct CosClient {
    bucket: String,
    region: String,
    base_url: String,
    client: reqwest::Client,
}

impl CosClient {
    pub fn new(bucket: String, region: String) -> Self {
        let base_url = format!("https://{}.cos.{}.myqcloud.com", bucket, region);
        
        Self {
            bucket,
            region,
            base_url,
            client: reqwest::Client::new(),
        }
    }

    /// 获取对象元数据 (HEAD 请求)
    pub async fn head_object(&self, key: &str) -> Result<ObjectMeta> {
        let url = format!("{}/{}", self.base_url, key);
        
        let response = self.client
            .head(&url)
            .send()
            .await?;

        if response.status() == 404 {
            return Err(anyhow!("Object not found: {}", key));
        }

        if !response.status().is_success() {
            return Err(anyhow!("HEAD request failed with status: {}", response.status()));
        }

        let headers = response.headers();
        
        let size = headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let last_modified = headers
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_http_date(v))
            .unwrap_or_else(SystemTime::now);

        let etag = headers
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        Ok(ObjectMeta {
            key: key.to_string(),
            size,
            last_modified,
            etag,
            content_type,
        })
    }

    /// 获取对象内容 (GET 请求)
    pub async fn get_object(&self, key: &str) -> Result<Bytes> {
        let url = format!("{}/{}", self.base_url, key);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;

        if response.status() == 404 {
            return Err(anyhow!("Object not found: {}", key));
        }

        if !response.status().is_success() {
            return Err(anyhow!("GET request failed with status: {}", response.status()));
        }

        let bytes = response.bytes().await?;
        Ok(bytes)
    }

    /// 列出所有对象 (简化版本，实际应该使用 COS 的 ListObjects API)
    /// 对于 demo，我们假设有一个预定义的对象列表
    pub async fn list_objects(&self) -> Result<Vec<String>> {
        // 这里应该调用 COS 的 ListObjects API
        // 为了 demo 简化，我们返回一些示例对象
        // 在实际使用中，你需要实现完整的 COS API 调用
        Ok(vec![
            "data/file1.txt".to_string(),
            "data/file2.jpg".to_string(),
            "data/subdir/file3.txt".to_string(),
            "README.md".to_string(),
        ])
    }
}

/// 简单的 HTTP 日期解析器
fn parse_http_date(_date_str: &str) -> Option<SystemTime> {
    // 这里应该实现完整的 HTTP 日期解析
    // 为了简化，返回当前时间
    Some(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cos_client_creation() {
        let client = CosClient::new("test-bucket".to_string(), "ap-beijing".to_string());
        assert_eq!(client.bucket, "test-bucket");
        assert_eq!(client.region, "ap-beijing");
    }
}
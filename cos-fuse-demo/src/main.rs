use clap::{Arg, Command};
use fuser::{MountOption, spawn_mount2};
use log::{error, info};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

mod cache;
mod cos_client;
mod filesystem;

use filesystem::CosFilesystem;

fn main() {
    // 初始化日志
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let matches = Command::new("cos-fuse-demo")
        .version("0.1.0")
        .about("A demo FUSE filesystem that mounts Tencent Cloud COS as a local filesystem")
        .arg(
            Arg::new("bucket")
                .short('b')
                .long("bucket")
                .value_name("BUCKET")
                .help("Tencent Cloud COS bucket name")
                .required(true),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("region")
                .value_name("REGION")
                .help("Tencent Cloud COS region (e.g., ap-beijing)")
                .required(true),
        )
        .arg(
            Arg::new("mount-point")
                .short('m')
                .long("mount-point")
                .value_name("MOUNT_POINT")
                .help("Directory to mount the filesystem")
                .required(true),
        )
        .arg(
            Arg::new("cache-dir")
                .short('c')
                .long("cache-dir")
                .value_name("CACHE_DIR")
                .help("Directory for file content cache")
                .default_value("/tmp/cosfs_cache"),
        )
        .arg(
            Arg::new("foreground")
                .short('f')
                .long("foreground")
                .help("Run in foreground")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .help("Enable debug logging")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    // 设置日志级别
    if matches.get_flag("debug") {
        log::set_max_level(log::LevelFilter::Debug);
    }

    let bucket = matches.get_one::<String>("bucket").unwrap().clone();
    let region = matches.get_one::<String>("region").unwrap().clone();
    let mount_point = matches.get_one::<String>("mount-point").unwrap().clone();
    let cache_dir = matches.get_one::<String>("cache-dir").unwrap().clone();
    let foreground = matches.get_flag("foreground");

    info!("Starting COS FUSE filesystem");
    info!("Bucket: {}", bucket);
    info!("Region: {}", region);
    info!("Mount point: {}", mount_point);
    info!("Cache directory: {}", cache_dir);

    // 验证挂载点
    let mount_path = PathBuf::from(&mount_point);
    if !mount_path.exists() {
        error!("Mount point does not exist: {}", mount_point);
        std::process::exit(1);
    }

    if !mount_path.is_dir() {
        error!("Mount point is not a directory: {}", mount_point);
        std::process::exit(1);
    }

    // 创建文件系统实例
    let cache_path = PathBuf::from(cache_dir);
    let fs = match CosFilesystem::new(bucket, region, &cache_path) {
        Ok(fs) => fs,
        Err(e) => {
            error!("Failed to create filesystem: {}", e);
            std::process::exit(1);
        }
    };

    // 检查挂载点是否为空目录
    let is_empty = match mount_path.read_dir() {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => {
            error!("Failed to read mount point directory: {}", e);
            std::process::exit(1);
        }
    };
    
    if !is_empty {
        error!("Mount point {} is not empty", mount_point);
        std::process::exit(1);
    }

    info!("Mounting filesystem...");

    // 设置挂载选项
    let options = vec![
        MountOption::RW,           // 读写模式（虽然我们只实现读）
        MountOption::FSName("cosfs".to_string()), // 文件系统名称
        MountOption::AutoUnmount,   // 自动卸载
        MountOption::AllowOther,   // 允许其他用户访问
    ];

    // 挂载文件系统
    match spawn_mount2(fs, &mount_path, &options) {
        Ok(_session) => {
            info!("Filesystem mounted successfully at {}", mount_point);
            
            if !foreground {
                info!("Running in background mode");
                return;
            }
            
            // 前台模式：等待信号
            info!("Running in foreground mode. Press Ctrl+C to unmount.");
            
            // 设置信号处理
            let (tx, rx) = std::sync::mpsc::channel();
            
            ctrlc::set_handler(move || {
                info!("Received Ctrl+C, unmounting...");
                let _ = tx.send(());
            })
            .expect("Error setting Ctrl-C handler");
            
            // 等待信号
            if rx.recv().is_ok() {
                info!("Unmounting filesystem...");
                // session 会在 drop 时自动卸载
            }
        }
        Err(e) => {
            error!("Failed to mount filesystem: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_line_parsing() {
        use clap::Parser;
        
        // 这里可以添加命令行解析的测试
    }
}
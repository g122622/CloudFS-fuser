#!/bin/bash

# COS FUSE Demo 测试脚本

set -e

echo "=== COS FUSE Demo 测试 ==="

# 检查是否已构建
if [ ! -f "target/debug/cos-fuse-demo" ]; then
    echo "构建项目..."
    cargo build
fi

# 创建测试挂载点
MOUNT_POINT="./mounted"
CACHE_DIR="./test_cache"

echo "创建测试目录..."
mkdir -p "$MOUNT_POINT"
mkdir -p "$CACHE_DIR"

echo "测试帮助信息..."
./target/debug/cos-fuse-demo --help

echo ""
echo "=== 测试完成 ==="
echo ""
echo "要运行实际的 FUSE 挂载，请使用以下命令："
echo "./target/debug/cos-fuse-demo --bucket your-bucket --region ap-beijing --mount-point $MOUNT_POINT --cache-dir $CACHE_DIR"
echo ""
echo "卸载命令："
echo "umount $MOUNT_POINT"
echo ""
echo "注意：实际运行需要配置有效的 COS 凭据"

./target/debug/cos-fuse-demo --bucket cloudfs-fuse-1319262409 --region ap-chongqing --mount-point $MOUNT_POINT --cache-dir $CACHE_DIR

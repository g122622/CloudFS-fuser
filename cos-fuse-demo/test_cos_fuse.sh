#!/bin/bash
# COS FUSE Demo 测试脚本
# 该脚本用于测试挂载的COS文件系统功能

echo "=== COS FUSE Demo 测试脚本 ==="

# 设置变量
MOUNT_POINT="/home/g122622/cos-mounted"
CACHE_DIR="/home/g122622/cos-cache"

echo "挂载点: $MOUNT_POINT"
echo "缓存目录: $CACHE_DIR"

# 1. 检查挂载点是否存在
echo
echo "1. 检查挂载点状态..."
if mountpoint -q "$MOUNT_POINT"; then
    echo "✓ 挂载点已正确挂载"
else
    echo "✗ 挂载点未挂载"
    exit 1
fi

# 2. 列出挂载点内容
echo
echo "2. 列出挂载点根目录内容..."
ls -la "$MOUNT_POINT"

# 3. 检查缓存目录
echo
echo "3. 检查缓存目录..."
if [ -d "$CACHE_DIR" ]; then
    echo "✓ 缓存目录存在"
    echo "缓存目录内容:"
    ls -la "$CACHE_DIR"
else
    echo "✗ 缓存目录不存在"
fi

# 4. 测试文件读取
echo
echo "4. 测试文件读取..."
# 查找第一个文件进行测试
FIRST_FILE=$(find "$MOUNT_POINT" -type f -not -path "*/\.*" | head -n 1)
if [ -n "$FIRST_FILE" ]; then
    echo "找到测试文件: $FIRST_FILE"
    echo "文件信息:"
    ls -lh "$FIRST_FILE"
    
    echo "尝试读取文件前10行:"
    head -n 10 "$FIRST_FILE" 2>/dev/null || echo "无法读取文件内容"
else
    echo "挂载点中未找到文件"
fi

# 5. 测试目录操作
echo
echo "5. 测试目录操作..."
TEST_DIR="$MOUNT_POINT/test_directory"
echo "创建测试目录: $TEST_DIR"
mkdir -p "$TEST_DIR" 2>/dev/null && echo "✓ 目录创建成功" || echo "⚠ 目录创建失败（可能没有写权限）"

if [ -d "$TEST_DIR" ]; then
    echo "目录已创建，正在删除..."
    rmdir "$TEST_DIR" 2>/dev/null && echo "✓ 目录删除成功" || echo "⚠ 目录删除失败"
fi

# 6. 检查文件系统使用情况
echo
echo "6. 检查文件系统使用情况..."
df -h "$MOUNT_POINT"

# 7. 显示FUSE相关信息
echo
echo "7. FUSE挂载信息..."
mount | grep fuse

echo
echo "=== 测试完成 ==="
echo "要卸载文件系统，请按 Ctrl+C"
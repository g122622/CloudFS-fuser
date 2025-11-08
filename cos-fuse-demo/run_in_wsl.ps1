# COS FUSE Demo - WSL 自动化脚本
# 该脚本在 Windows 环境中运行，自动在 WSL 中完成编译、文件复制和启动项目

param(
    [string]$Bucket = "cloudfs-fuse-1319262409",
    [string]$Region = "ap-chongqing"
)

# 设置项目路径
$ProjectDir = "D:\RustProjects\CloudFS-fuser\cos-fuse-demo"
$WSLUsername = "g122622"
$WSLProjectDir = "/home/${WSLUsername}/cos-fuse-demo"

Write-Host "=== COS FUSE Demo WSL 自动化脚本 ===" -ForegroundColor Green
Write-Host "项目目录: $ProjectDir" -ForegroundColor Yellow
Write-Host "WSL用户名: $WSLUsername" -ForegroundColor Yellow
Write-Host "WSL项目目录: $WSLProjectDir" -ForegroundColor Yellow

# 检查项目目录是否存在
if (-not (Test-Path $ProjectDir)) {
    Write-Error "项目目录不存在: $ProjectDir"
    exit 1
}

# 1. 确保WSL中已安装必要的依赖
Write-Host "1. 检查并安装WSL依赖..." -ForegroundColor Cyan
wsl -d Ubuntu -u $WSLUsername -e bash -c "sudo apt update && sudo apt install -y libfuse-dev pkg-config curl build-essential"

# 2. 检查Rust安装
Write-Host "2. 检查Rust安装..." -ForegroundColor Cyan
$rustVersion = wsl -d Ubuntu -u $WSLUsername -e bash -c "source ~/.cargo/env 2>/dev/null && rustc --version || echo 'Rust not found'"
Write-Host "Rust版本: $rustVersion" -ForegroundColor Yellow

# 3. 清理可能存在的旧项目目录并重新创建
Write-Host "3. 清理并准备WSL项目目录..." -ForegroundColor Cyan
wsl -d Ubuntu -u $WSLUsername -e bash -c "rm -rf $WSLProjectDir && mkdir -p $WSLProjectDir"

# 4. 将项目文件复制到WSL原生文件系统
Write-Host "4. 将项目文件复制到WSL..." -ForegroundColor Cyan
# 使用tar方法替代robocopy以避免权限问题
wsl -d Ubuntu -u $WSLUsername -e bash -c "cd /mnt/d/RustProjects/CloudFS-fuser/cos-fuse-demo && tar -cf - . | (cd $WSLProjectDir && tar -xf -)"

# 5. 修复文件权限
Write-Host "5. 修复文件权限..." -ForegroundColor Cyan
wsl -d Ubuntu -u $WSLUsername -e bash -c "chmod -R 755 $WSLProjectDir"

# 6. 在WSL中构建项目
Write-Host "6. 在WSL中构建项目..." -ForegroundColor Cyan
$buildResult = wsl -d Ubuntu -u $WSLUsername -e bash -c "cd $WSLProjectDir && source ~/.cargo/env 2>/dev/null && CARGO_TARGET_DIR=target cargo build"
if ($LASTEXITCODE -ne 0) {
    Write-Warning "第一次构建失败，尝试清理后再构建..."
    # 清理并重新构建
    wsl -d Ubuntu -u $WSLUsername -e bash -c "cd $WSLProjectDir && source ~/.cargo/env 2>/dev/null && cargo clean && CARGO_TARGET_DIR=target cargo build"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "构建失败"
        exit 1
    }
}
Write-Host "构建成功完成!" -ForegroundColor Green

# 7. 准备挂载点和缓存目录
Write-Host "7. 准备挂载点和缓存目录..." -ForegroundColor Cyan

# 终止可能残留的进程
wsl -d Ubuntu -u $WSLUsername -e bash -c "pkill -f cos-fuse-demo 2>/dev/null || true"
Start-Sleep -Seconds 1

# 尝试卸载（正常 + lazy）
wsl -d Ubuntu -u $WSLUsername -e bash -c "fusermount -u ~/cos-mounted 2>/dev/null; sudo umount -l ~/cos-mounted 2>/dev/null || true"
Start-Sleep -Seconds 1

# 安全清理目录
wsl -d Ubuntu -u $WSLUsername -e bash -c @"
    # 如果存在，先重命名避免直接操作坏挂载点
    if [ -e ~/cos-mounted ]; then
        mv ~/cos-mounted ~/cos-mounted.DELETING 2>/dev/null || true
    fi
    rm -rf ~/cos-mounted.DELETING &
    rm -rf ~/cos-cache
    mkdir -p ~/cos-mounted ~/cos-cache
"@

Write-Host "  挂载点和缓存目录已准备就绪" -ForegroundColor Green

# 8. 确保/etc/fuse.conf中有user_allow_other配置
Write-Host "8. 配置FUSE..." -ForegroundColor Cyan
wsl -d Ubuntu -u $WSLUsername -e bash -c "grep -q 'user_allow_other' /etc/fuse.conf || echo 'user_allow_other' | sudo tee -a /etc/fuse.conf"

# 9. 启动项目
Write-Host "9. 启动项目..." -ForegroundColor Cyan
Write-Host "挂载点: /home/${WSLUsername}/cos-mounted" -ForegroundColor Yellow
Write-Host "缓存目录: /home/${WSLUsername}/cos-cache" -ForegroundColor Yellow
Write-Host "要卸载文件系统，请在WSL终端中按 Ctrl+C" -ForegroundColor Yellow

# 启动项目（前台模式）
Write-Host "正在启动项目..." -ForegroundColor Cyan
wsl -d Ubuntu -u $WSLUsername -e bash -c "cd $WSLProjectDir && source ~/.cargo/env 2>/dev/null && ./target/debug/cos-fuse-demo --bucket $Bucket --region $Region --mount-point ~/cos-mounted --cache-dir ~/cos-cache --foreground"
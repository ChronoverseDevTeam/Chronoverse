#!/usr/bin/env pwsh
# Chronoverse 测试启动脚本

param(
    [Parameter(Position=0)]
    [ValidateSet("all", "unit", "grpc", "workspace", "user", "auth", "list")]
    [string]$TestType = "all",
    
    [switch]$NoCaptureOutput,
    [switch]$ShowOutput,
    [switch]$ShowBacktrace
)

$ErrorActionPreference = "Stop"

Write-Host "=====================================" -ForegroundColor Cyan
Write-Host "  Chronoverse 测试启动脚本" -ForegroundColor Cyan
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host ""

# 检查 MongoDB 是否运行
function Test-MongoDBRunning {
    Write-Host "检查 MongoDB 连接..." -ForegroundColor Yellow
    
    # 尝试连接 MongoDB
    $mongoRunning = $false
    try {
        $tcpClient = New-Object System.Net.Sockets.TcpClient
        $tcpClient.Connect("127.0.0.1", 27017)
        $tcpClient.Close()
        $mongoRunning = $true
    } catch {
        $mongoRunning = $false
    }
    
    if ($mongoRunning) {
        Write-Host "✓ MongoDB 正在运行" -ForegroundColor Green
        return $true
    } else {
        Write-Host "✗ MongoDB 未运行" -ForegroundColor Red
        Write-Host ""
        Write-Host "请先启动 MongoDB:" -ForegroundColor Yellow
        Write-Host "  cd crv-hive/mongo" -ForegroundColor White
        Write-Host "  .\native_start.ps1" -ForegroundColor White
        Write-Host ""
        return $false
    }
}

# 构建测试命令
function Get-TestCommand {
    param([string]$Type)
    
    $baseCmd = "cargo test --package crv-hive --lib"
    
    # 添加输出选项（默认单线程顺序执行）
    $outputFlags = "-- --test-threads=1"
    if ($ShowOutput) {
        $outputFlags = "-- --test-threads=1 --show-output"
    } elseif ($NoCaptureOutput) {
        $outputFlags = "-- --test-threads=1 --nocapture"
    }
    
    switch ($Type) {
        "all" {
            return "$baseCmd $outputFlags"
        }
        "grpc" {
            return "$baseCmd test_grpc $outputFlags"
        }
        "workspace" {
            return "$baseCmd workspace $outputFlags"
        }
        "user" {
            return "$baseCmd test_user $outputFlags"
        }
        "auth" {
            return "$baseCmd test_grpc_auth $outputFlags"
        }
        "list" {
            return "$baseCmd list_workspaces $outputFlags"
        }
        "unit" {
            return "$baseCmd --lib $outputFlags"
        }
    }
}

# 显示测试信息
function Show-TestInfo {
    param([string]$Type)
    
    Write-Host "测试类型: " -NoNewline -ForegroundColor White
    Write-Host $Type -ForegroundColor Green
    Write-Host ""
    
    switch ($Type) {
        "all" {
            Write-Host "运行所有测试（包括 gRPC、workspace、user 等）" -ForegroundColor Gray
        }
        "grpc" {
            Write-Host "运行所有 gRPC 集成测试" -ForegroundColor Gray
            Write-Host "  - test_grpc_greeting" -ForegroundColor DarkGray
            Write-Host "  - test_grpc_auth_flow" -ForegroundColor DarkGray
            Write-Host "  - test_grpc_workspace_operations" -ForegroundColor DarkGray
            Write-Host "  - test_grpc_auth_validation" -ForegroundColor DarkGray
            Write-Host "  - test_grpc_workspace_validation" -ForegroundColor DarkGray
        }
        "workspace" {
            Write-Host "运行 workspace 相关测试" -ForegroundColor Gray
        }
        "user" {
            Write-Host "运行 user 相关测试" -ForegroundColor Gray
        }
        "auth" {
            Write-Host "运行认证相关测试" -ForegroundColor Gray
        }
        "list" {
            Write-Host "运行 list_workspaces 测试" -ForegroundColor Gray
        }
    }
    Write-Host ""
}

# 运行所有测试（串行执行每个测试）
function Run-AllTests {
    $testNames = @(
        "test_auth_logic_flow",
        "test_auth_validation",
        "test_workspace_validation",
        "test_workspace_logic_operations",
        "test_user_crud",
        "test_workspace_crud_and_query",
        "test_list_workspaces_logic"
    )
    
    $passedCount = 0
    $failedCount = 0
    $failedTests = @()
    
    Write-Host "串行执行所有测试..." -ForegroundColor Cyan
    Write-Host ""
    
    foreach ($testName in $testNames) {
        Write-Host "运行: $testName" -ForegroundColor Yellow
        
        $outputFlags = ""
        if ($ShowOutput) {
            $outputFlags = "-- --show-output"
        } elseif ($NoCaptureOutput) {
            $outputFlags = "-- --nocapture"
        }
        
        $cmd = "cargo test --package crv-hive --lib $testName $outputFlags"
        Invoke-Expression $cmd
        
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  ✓ $testName 通过" -ForegroundColor Green
            $passedCount++
        } else {
            Write-Host "  ✗ $testName 失败" -ForegroundColor Red
            $failedCount++
            $failedTests += $testName
        }
        Write-Host ""
    }
    
    Write-Host "=====================================" -ForegroundColor Cyan
    Write-Host "测试汇总:" -ForegroundColor White
    Write-Host "  通过: $passedCount" -ForegroundColor Green
    Write-Host "  失败: $failedCount" -ForegroundColor Red
    
    if ($failedCount -gt 0) {
        Write-Host ""
        Write-Host "失败的测试:" -ForegroundColor Red
        foreach ($test in $failedTests) {
            Write-Host "  - $test" -ForegroundColor Red
        }
        return 1
    }
    
    return 0
}

# 主函数
function Main {
    # 检查是否在项目根目录
    if (-not (Test-Path "Cargo.toml")) {
        Write-Host "错误: 请在项目根目录运行此脚本" -ForegroundColor Red
        exit 1
    }
    
    # 检查 MongoDB
    if (-not (Test-MongoDBRunning)) {
        Write-Host ""
        $response = Read-Host "是否继续运行测试? (测试可能会失败) [y/N]"
        if ($response -ne "y" -and $response -ne "Y") {
            Write-Host "测试已取消" -ForegroundColor Yellow
            exit 0
        }
    }
    
    Write-Host ""
    Show-TestInfo $TestType
    
    # 运行测试
    if ($ShowBacktrace) {
        $env:RUST_BACKTRACE = "1"
    }
    
    # 如果是运行所有测试，使用串行执行
    if ($TestType -eq "all") {
        Write-Host "执行模式: 串行执行每个测试（避免缓存冲突）" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "=====================================" -ForegroundColor Cyan
        Write-Host ""
        
        $exitCode = Run-AllTests
        
        Write-Host ""
        Write-Host "=====================================" -ForegroundColor Cyan
        
        if ($exitCode -eq 0) {
            Write-Host "✓ 所有测试通过" -ForegroundColor Green
        } else {
            Write-Host "✗ 部分测试失败" -ForegroundColor Red
        }
        
        exit $exitCode
    } else {
        # 运行特定类型的测试
        $testCmd = Get-TestCommand $TestType
        
        Write-Host "执行命令: " -NoNewline -ForegroundColor White
        Write-Host $testCmd -ForegroundColor Cyan
        Write-Host ""
        Write-Host "=====================================" -ForegroundColor Cyan
        Write-Host ""
        
        Invoke-Expression $testCmd
        $exitCode = $LASTEXITCODE
        
        Write-Host ""
        Write-Host "=====================================" -ForegroundColor Cyan
        
        if ($exitCode -eq 0) {
            Write-Host "✓ 测试通过" -ForegroundColor Green
        } else {
            Write-Host "✗ 测试失败" -ForegroundColor Red
        }
        
        exit $exitCode
    }
}

# 显示帮助信息
function Show-Help {
    Write-Host "用法: .\test.ps1 [测试类型] [选项]" -ForegroundColor White
    Write-Host ""
    Write-Host "测试类型:" -ForegroundColor Yellow
    Write-Host "  all         运行所有测试 (默认)" -ForegroundColor White
    Write-Host "  grpc        运行 gRPC 集成测试" -ForegroundColor White
    Write-Host "  workspace   运行 workspace 测试" -ForegroundColor White
    Write-Host "  user        运行 user 测试" -ForegroundColor White
    Write-Host "  auth        运行认证测试" -ForegroundColor White
    Write-Host "  list        运行 list_workspaces 测试" -ForegroundColor White
    Write-Host "  unit        运行单元测试" -ForegroundColor White
    Write-Host ""
    Write-Host "选项:" -ForegroundColor Yellow
    Write-Host "  -NoCaptureOutput   不捕获测试输出 (显示 println!)" -ForegroundColor White
    Write-Host "  -ShowOutput        显示成功测试的输出" -ForegroundColor White
    Write-Host "  -ShowBacktrace     显示详细的错误信息（包括 backtrace）" -ForegroundColor White
    Write-Host ""
    Write-Host "示例:" -ForegroundColor Yellow
    Write-Host "  .\test.ps1                        # 运行所有测试" -ForegroundColor Gray
    Write-Host "  .\test.ps1 grpc                   # 运行 gRPC 测试" -ForegroundColor Gray
    Write-Host "  .\test.ps1 grpc -ShowBacktrace    # 运行 gRPC 测试并显示详细信息" -ForegroundColor Gray
    Write-Host "  .\test.ps1 workspace -ShowOutput  # 运行 workspace 测试并显示输出" -ForegroundColor Gray
    Write-Host ""
}

# 检查是否请求帮助
if ($args -contains "-h" -or $args -contains "--help" -or $args -contains "help") {
    Show-Help
    exit 0
}

# 运行主函数
Main


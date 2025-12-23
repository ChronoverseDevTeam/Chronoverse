Param(
  [string]$Command = "all",
  [string]$Workspace = "test-workspace",
  [string]$WorkspaceRoot = "D:\test-workspace",
  [switch]$Silent
)

$ErrorActionPreference = "Stop"

function Write-Info($msg) { if (-not $Silent) { Write-Host "[INFO] $msg" -ForegroundColor Cyan } }
function Write-Ok($msg) { if (-not $Silent) { Write-Host "[ OK ] $msg" -ForegroundColor Green } }
function Write-Warn($msg) { if (-not $Silent) { Write-Host "[WARN] $msg" -ForegroundColor Yellow } }
function Write-Error-Custom($msg) { Write-Host "[ERROR] $msg" -ForegroundColor Red }

$projectRoot = Split-Path -Parent $PSScriptRoot
$cliExe = Join-Path $projectRoot "target\debug\crv-cli.exe"

# 检查 CLI 是否已编译
if (-not (Test-Path $cliExe)) {
  Write-Info "crv-cli 未找到，正在编译..."
  Push-Location $projectRoot
  try {
    cargo build --package crv-cli
    if ($LASTEXITCODE -ne 0) {
      throw "编译 crv-cli 失败"
    }
  } finally {
    Pop-Location
  }
}

Write-Ok "使用 CLI: $cliExe"

# 测试函数
function Test-EdgeBonjour {
  Write-Info "测试: edge bonjour"
  & $cliExe edge bonjour
  if ($LASTEXITCODE -eq 0) {
    Write-Ok "edge bonjour 成功"
  } else {
    Write-Error-Custom "edge bonjour 失败"
  }
}

function Test-EdgeBonjourHive {
  Write-Info "测试: edge bonjour-hive"
  # 注意：这个命令可能需要 hive 服务运行
  Write-Warn "需要 hive 服务运行"
}

function Test-WorkspaceCreate {
  Write-Info "测试: workspace create"
  Write-Info "工作区名称: $Workspace"
  Write-Info "工作区路径: $WorkspaceRoot"
  # 注意：此命令是交互式的，这里仅展示用法
  # & $cliExe workspace create
  Write-Warn "workspace create 是交互式命令，请手动测试"
  Write-Info "命令示例: $cliExe workspace create"
}

function Test-WorkspaceList {
  Write-Info "测试: workspace list"
  & $cliExe workspace list
  if ($LASTEXITCODE -eq 0) {
    Write-Ok "workspace list 成功"
  } else {
    Write-Error-Custom "workspace list 失败"
  }
}

function Test-Add {
  Write-Info "测试: add"
  $testFiles = @("README.md", "src/main.rs")
  Write-Info "添加文件: $($testFiles -join ', ')"
  Write-Info "命令示例: $cliExe add -w $Workspace $($testFiles -join ' ')"
  # & $cliExe add -w $Workspace $testFiles
  Write-Warn "需要有效的工作区和文件路径"
}

function Test-Delete {
  Write-Info "测试: delete"
  $testFiles = @("README.md")
  Write-Info "删除文件: $($testFiles -join ', ')"
  Write-Info "命令示例: $cliExe delete -w $Workspace $($testFiles -join ' ')"
  # & $cliExe delete -w $Workspace $testFiles
  Write-Warn "需要有效的工作区和文件路径"
}

function Test-Sync {
  Write-Info "测试: sync"
  $testPaths = @("//...")
  Write-Info "同步路径: $($testPaths -join ', ')"
  Write-Info "命令示例: $cliExe sync -w $Workspace $($testPaths -join ' ')"
  # & $cliExe sync -w $Workspace $testPaths
  Write-Warn "需要有效的工作区和 hive 服务"
}

function Test-Submit {
  Write-Info "测试: submit"
  $testPaths = @("//...")
  $description = "Test submit"
  Write-Info "提交路径: $($testPaths -join ', ')"
  Write-Info "描述: $description"
  Write-Info "命令示例: $cliExe submit -w $Workspace -d '$description' $($testPaths -join ' ')"
  # & $cliExe submit -w $Workspace -d $description $testPaths
  Write-Warn "需要有效的工作区、文件和 hive 服务"
}

function Test-ChangelistCreate {
  Write-Info "测试: changelist create"
  Write-Warn "changelist create 是交互式命令，请手动测试"
  Write-Info "命令示例: $cliExe changelist create"
}

function Test-ChangelistList {
  Write-Info "测试: changelist list"
  Write-Info "命令示例: $cliExe changelist list -w $Workspace"
  # & $cliExe changelist list -w $Workspace
  Write-Warn "需要有效的工作区"
}

# 主测试流程
Write-Info "=== CRV CLI 测试脚本 ==="
Write-Info "命令: $Command"
Write-Info ""

switch ($Command.ToLower()) {
  "all" {
    Write-Info "运行所有测试..."
    Test-EdgeBonjour
    Write-Info ""
    Test-WorkspaceList
    Write-Info ""
    Test-Add
    Write-Info ""
    Test-Delete
    Write-Info ""
    Test-Sync
    Write-Info ""
    Test-Submit
    Write-Info ""
    Test-ChangelistList
  }
  "edge" {
    Test-EdgeBonjour
  }
  "edge-bonjour" {
    Test-EdgeBonjour
  }
  "edge-bonjour-hive" {
    Test-EdgeBonjourHive
  }
  "workspace" {
    Test-WorkspaceList
  }
  "workspace-create" {
    Test-WorkspaceCreate
  }
  "workspace-list" {
    Test-WorkspaceList
  }
  "add" {
    Test-Add
  }
  "delete" {
    Test-Delete
  }
  "sync" {
    Test-Sync
  }
  "submit" {
    Test-Submit
  }
  "changelist" {
    Test-ChangelistList
  }
  "changelist-create" {
    Test-ChangelistCreate
  }
  "changelist-list" {
    Test-ChangelistList
  }
  default {
    Write-Error-Custom "未知命令: $Command"
    Write-Info ""
    Write-Info "可用命令:"
    Write-Info "  all                  - 运行所有测试"
    Write-Info "  edge                 - 测试 edge bonjour"
    Write-Info "  edge-bonjour         - 测试 edge bonjour"
    Write-Info "  edge-bonjour-hive    - 测试 edge bonjour-hive"
    Write-Info "  workspace            - 测试 workspace list"
    Write-Info "  workspace-create     - 测试 workspace create"
    Write-Info "  workspace-list       - 测试 workspace list"
    Write-Info "  add                  - 测试 add"
    Write-Info "  delete               - 测试 delete"
    Write-Info "  sync                 - 测试 sync"
    Write-Info "  submit               - 测试 submit"
    Write-Info "  changelist           - 测试 changelist list"
    Write-Info "  changelist-create    - 测试 changelist create"
    Write-Info "  changelist-list      - 测试 changelist list"
    Write-Info ""
    Write-Info "参数:"
    Write-Info "  -Workspace <name>    - 指定工作区名称 (默认: test-workspace)"
    Write-Info "  -WorkspaceRoot <path> - 指定工作区路径 (默认: D:\test-workspace)"
    Write-Info "  -Silent              - 静默模式"
    Write-Info ""
    Write-Info "示例:"
    Write-Info "  .\test.ps1 -Command edge"
    Write-Info "  .\test.ps1 -Command add -Workspace myworkspace"
    Write-Info "  .\test.ps1 -Command all -Silent"
    exit 1
  }
}

Write-Info ""
Write-Ok "测试完成！"


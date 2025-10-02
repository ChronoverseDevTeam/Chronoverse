Param(
  [switch]$Recreate,
  [switch]$Silent
)

$ErrorActionPreference = "Stop"

function Write-Info($msg) { if (-not $Silent) { Write-Host "[INFO] $msg" -ForegroundColor Cyan } }
function Write-Ok($msg) { if (-not $Silent) { Write-Host "[ OK ] $msg" -ForegroundColor Green } }
function Write-Warn($msg) { if (-not $Silent) { Write-Host "[WARN] $msg" -ForegroundColor Yellow } }

$composeFile = Join-Path $PSScriptRoot "docker-compose.yml"
if (-not (Test-Path $composeFile)) { throw "未找到 docker-compose.yml: $composeFile" }

# Ensure data/init directories exist (bind mounts)
$dataDir = Join-Path $PSScriptRoot "data"
$initDir = Join-Path $PSScriptRoot "init"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
New-Item -ItemType Directory -Force -Path $initDir | Out-Null

# Check docker availability
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
  throw "未检测到 Docker，请先安装并启动 Docker Desktop"
}

# Use compose v2 syntax: docker compose
function Invoke-Compose([string[]]$args) {
  $cmd = "docker compose $($args -join ' ')"
  Write-Info $cmd
  $p = Start-Process -FilePath "docker" -ArgumentList @("compose") + $args -NoNewWindow -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "docker compose 命令失败: $cmd" }
}

if ($Recreate) {
  Write-Info "清理现有容器..."
  Invoke-Compose @("-f", $composeFile, "down", "--remove-orphans", "--volumes")
}

Write-Info "启动 MongoDB 容器..."
Invoke-Compose @("-f", $composeFile, "up", "-d")

# Wait for healthy
$container = "crv-hive-mongo"
$maxWaitSec = 60
$elapsed = 0
Write-Info "等待容器健康检查通过(<= $maxWaitSec 秒)..."
while ($true) {
  $status = (docker inspect -f '{{.State.Health.Status}}' $container 2>$null)
  if ($LASTEXITCODE -ne 0) {
    Write-Warn "无法获取容器健康状态，继续等待..."
  } elseif ($status -eq "healthy") {
    break
  } elseif ($status -eq "unhealthy") {
    throw "容器健康检查失败"
  }
  Start-Sleep -Seconds 2
  $elapsed += 2
  if ($elapsed -ge $maxWaitSec) { throw "等待超时：容器未在 $maxWaitSec 秒内变为 healthy" }
}

Write-Ok "MongoDB 已就绪。"
Write-Host "连接信息:" -ForegroundColor Magenta
Write-Host "  URI: mongodb://127.0.0.1:27017" -ForegroundColor Magenta
Write-Host "  数据库: chronoverse" -ForegroundColor Magenta
Write-Host "提示: 默认无鉴权，对应当前配置 \"mongo_username\" 为 None。" -ForegroundColor DarkGray



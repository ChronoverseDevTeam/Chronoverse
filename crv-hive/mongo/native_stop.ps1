Param(
  [switch]$Force
)

$ErrorActionPreference = "Stop"

$baseDir = $PSScriptRoot
$pidFile = Join-Path $baseDir "mongod.pid"

function Is-Process-Alive([int]$processId) {
  try { $p = Get-Process -Id $processId -ErrorAction Stop; return $true } catch { return $false }
}

if (-not (Test-Path $pidFile)) {
  Write-Host "未找到 PID 文件，可能未启动或已停止。" -ForegroundColor Yellow
  return
}

$processId = [int](Get-Content $pidFile -Raw).Trim()
if ($processId -le 0 -or -not (Is-Process-Alive $processId)) {
  Write-Host "PID 无效或进程不存在，清理 PID 文件。" -ForegroundColor Yellow
  Remove-Item $pidFile -Force
  return
}

try {
  if ($Force) {
    Stop-Process -Id $processId -Force -ErrorAction Stop
  } else {
    Stop-Process -Id $processId -ErrorAction Stop
  }
  Write-Host "已停止 MongoDB 进程 (PID=$processId)" -ForegroundColor Green
} finally {
  if (Test-Path $pidFile) { Remove-Item $pidFile -Force }
}



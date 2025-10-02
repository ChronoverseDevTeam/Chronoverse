$ErrorActionPreference = "Stop"

$composeFile = Join-Path $PSScriptRoot "docker-compose.yml"
if (-not (Test-Path $composeFile)) { throw "未找到 docker-compose.yml: $composeFile" }

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
  throw "未检测到 Docker，请先安装并启动 Docker Desktop"
}

function Invoke-Compose([string[]]$args) {
  $p = Start-Process -FilePath "docker" -ArgumentList @("compose") + $args -NoNewWindow -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "docker compose 命令失败" }
}

Param(
  [switch]$RemoveVolumes
)

if ($RemoveVolumes) {
  Invoke-Compose @("-f", $composeFile, "down", "--remove-orphans", "--volumes")
} else {
  Invoke-Compose @("-f", $composeFile, "down", "--remove-orphans")
}

Write-Host "MongoDB 已停止" -ForegroundColor Green



Param(
  [string]$Version = "8.2.0",
  [int]$Port = 27017,
  [switch]$RecreateData,
  [switch]$Silent
)

$ErrorActionPreference = "Stop"

function Write-Info($msg) { if (-not $Silent) { Write-Host "[INFO] $msg" -ForegroundColor Cyan } }
function Write-Ok($msg) { if (-not $Silent) { Write-Host "[ OK ] $msg" -ForegroundColor Green } }
function Write-Warn($msg) { if (-not $Silent) { Write-Host "[WARN] $msg" -ForegroundColor Yellow } }

$baseDir = $PSScriptRoot
$binDir = Join-Path $baseDir "bin"
$distDir = Join-Path $baseDir "dist"
$dataDir = Join-Path $baseDir "data-native"
$logsDir = Join-Path $baseDir "logs"
$pidFile = Join-Path $baseDir "mongod.pid"
$logFile = Join-Path $logsDir "mongod.log"

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
New-Item -ItemType Directory -Force -Path $logsDir | Out-Null

if ($RecreateData -and (Test-Path $dataDir)) {
  Write-Info "清空数据目录: $dataDir"
  Remove-Item -Recurse -Force $dataDir
}
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

function Is-Process-Alive([int]$processId) {
  try { $p = Get-Process -Id $processId -ErrorAction Stop; return $true } catch { return $false }
}

if (Test-Path $pidFile) {
  $existingProcessId = [int](Get-Content $pidFile -Raw).Trim()
  if ($existingProcessId -gt 0 -and (Is-Process-Alive $existingProcessId)) {
    Write-Ok "MongoDB 已在运行 (PID=$existingProcessId)"
    Write-Host "URI: mongodb://127.0.0.1:$Port" -ForegroundColor Magenta
    return
  } else {
    Write-Warn "发现失效的 PID 文件，已删除"
    Remove-Item $pidFile -Force
  }
}

$mongodExe = Join-Path $binDir "mongod.exe"
if (-not (Test-Path $mongodExe)) {
  $zipName = "mongodb-windows-x86_64-$Version.zip"
  $url = "https://fastdl.mongodb.org/windows/$zipName"
  $zipPath = Join-Path $distDir $zipName
  Write-Info "下载 MongoDB ${Version}: ${url}"
  try {
    Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
  } catch {
    throw "下载失败：$url。可通过 -Version 指定其他版本，如 6.0.15 或 7.0.x"
  }

  $extractDir = Join-Path $distDir ("mongodb-$Version")
  Write-Info "解压到: $extractDir"
  if (Test-Path $extractDir) { Remove-Item -Recurse -Force $extractDir }
  Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

  $candidateBin = Join-Path $extractDir ("mongodb-windows-x86_64-$Version/bin")
  if (-not (Test-Path $candidateBin)) {
    # 某些版本目录名可能不同，回退到搜索
    $binSearch = Get-ChildItem -Path $extractDir -Recurse -Filter "mongod.exe" | Select-Object -First 1
    if (-not $binSearch) { throw "未在解压目录找到 mongod.exe" }
    $candidateBin = Split-Path -Path $binSearch.FullName -Parent
  }

  Write-Info "复制 mongod 可执行文件到: $binDir"
  Copy-Item -Path (Join-Path $candidateBin "*") -Destination $binDir -Recurse -Force
}

if (-not (Test-Path $mongodExe)) { throw "未找到 mongod.exe: $mongodExe" }

Write-Info "启动 mongod (端口 $Port)..."
$mongodArgs = @(
  "--dbpath", $dataDir,
  "--port", "$Port",
  "--bind_ip", "127.0.0.1",
  "--logpath", $logFile,
  "--logappend"
)
$proc = Start-Process -FilePath $mongodExe -ArgumentList $mongodArgs -WorkingDirectory $baseDir -WindowStyle Hidden -PassThru
Set-Content -Path $pidFile -Value $proc.Id

function Test-TcpPort([string]$targetHost, [int]$targetPort) {
  try {
    $client = New-Object System.Net.Sockets.TcpClient
    $iar = $client.BeginConnect($targetHost, $targetPort, $null, $null)
    $ok = $iar.AsyncWaitHandle.WaitOne(2000)
    if ($ok -and $client.Connected) { $client.Close(); return $true }
    $client.Close(); return $false
  } catch { return $false }
}

Write-Info "等待 MongoDB 端口就绪..."
$maxWait = 60
$elapsed = 0
while (-not (Test-TcpPort "127.0.0.1" $Port)) {
  Start-Sleep -Seconds 2
  $elapsed += 2
  if ($elapsed -ge $maxWait) {
    Write-Warn "等待端口就绪超时 ($maxWait 秒)，请查看日志: $logFile"
    break
  }
}

Write-Ok "MongoDB 已启动 (PID=$($proc.Id))。"
Write-Host "连接信息:" -ForegroundColor Magenta
Write-Host "  URI: mongodb://127.0.0.1:$Port" -ForegroundColor Magenta
Write-Host "  数据库: chronoverse" -ForegroundColor Magenta
Write-Host "日志: $logFile" -ForegroundColor DarkGray
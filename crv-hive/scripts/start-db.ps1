#!/usr/bin/env pwsh

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$composeFile = Join-Path $projectRoot "docker-compose.yml"
$envFile = Join-Path $projectRoot ".env"

if (-not (Test-Path $composeFile)) {
    throw "docker-compose.yml not found at $composeFile"
}

if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        if ($_ -match '^\s*#' -or $_ -match '^\s*$') {
            return
        }

        $name, $value = $_ -split '=', 2
        if ($name -and $value) {
            Set-Item -Path "Env:$name" -Value $value
        }
    }
}

if (Get-Command docker-compose -ErrorAction SilentlyContinue) {
    $composeCmd = "docker-compose"
    $composeArgs = @("-f", $composeFile)
} elseif (Get-Command docker -ErrorAction SilentlyContinue) {
    $composeCmd = "docker"
    $composeArgs = @("compose", "-f", $composeFile)
} else {
    throw "Neither docker-compose nor docker is available in PATH."
}

& $composeCmd @composeArgs up -d postgres
if ($LASTEXITCODE -ne 0) {
    throw "Failed to start Postgres with Docker Compose."
}

$attempts = 30
for ($i = 1; $i -le $attempts; $i++) {
    $status = docker inspect -f "{{.State.Health.Status}}" crv-hive-postgres 2>$null
    if ($status -eq "healthy") {
        Write-Host "Postgres is healthy." -ForegroundColor Green
        exit 0
    }

    Start-Sleep -Seconds 2
}

throw "Postgres did not become healthy in time."
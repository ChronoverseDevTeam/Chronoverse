#!/usr/bin/env pwsh

param(
    [switch]$Ignored,
    [switch]$NoCapture,
    [string]$Filter = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir

& (Join-Path $scriptDir "start-db.ps1")

$envFile = Join-Path $projectRoot ".env"
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

if (-not $env:TEST_DATABASE_URL) {
    $env:TEST_DATABASE_URL = "postgres://crv:crv@127.0.0.1:55432/chronoverse_test"
}

$env:DATABASE_URL = $env:TEST_DATABASE_URL
$env:CRV_RUN_HIVE_DB_TESTS = "1"

$args = @("test", "-p", "crv-hive", "--lib", "--tests")
if ($Filter) {
    $args += $Filter
}

$args += "--"
if ($Ignored) {
    $args += "--ignored"
}
if ($NoCapture) {
    $args += "--nocapture"
}

Push-Location $projectRoot
try {
    cargo @args
}
finally {
    Pop-Location
}
#!/usr/bin/env pwsh

param(
    [switch]$Ignored,
    [switch]$NoCapture,
    [string]$Filter = "",
    [string]$Config = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$configPath = if ($Config) { $Config } else { Join-Path $projectRoot "hive.example.toml" }

function Get-TomlValue {
    param(
        [string]$Path,
        [string]$Section,
        [string]$Key
    )

    $currentSection = ""
    foreach ($line in Get-Content $Path) {
        $trimmed = $line.Trim()
        if (-not $trimmed -or $trimmed.StartsWith("#")) {
            continue
        }

        if ($trimmed -match '^\[(.+)\]$') {
            $currentSection = $matches[1]
            continue
        }

        $pattern = '^{0}\s*=\s*([''\"])(.*)\1\s*$' -f [regex]::Escape($Key)
        if ($currentSection -eq $Section -and $trimmed -match $pattern) {
            return $matches[2]
        }
    }

    throw "missing [$Section].$Key in $Path"
}

& (Join-Path $scriptDir "start-db.ps1") -Config $configPath

$env:DATABASE_URL = Get-TomlValue -Path $configPath -Section "database" -Key "test_url"
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
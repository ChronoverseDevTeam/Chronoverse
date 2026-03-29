#!/usr/bin/env pwsh

param(
    [string]$Config = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$composeFile = Join-Path $projectRoot "docker-compose.yml"
$configPath = if ($Config) { $Config } else { Join-Path $projectRoot "hive.example.toml" }

if (-not (Test-Path $composeFile)) {
    throw "docker-compose.yml not found at $composeFile"
}

if (-not (Test-Path $configPath)) {
    throw "config file not found at $configPath"
}

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

function Parse-PostgresUrl {
    param([string]$Url)

    $uri = [Uri]$Url
    if ($uri.Scheme -ne "postgres") {
        throw "unsupported database url scheme: $Url"
    }

    $userInfo = $uri.UserInfo.Split(":", 2)
    if ($userInfo.Count -ne 2) {
        throw "database url must contain username and password: $Url"
    }

    $database = $uri.AbsolutePath.TrimStart("/")
    if (-not $database) {
        throw "database url must contain database name: $Url"
    }

    [pscustomobject]@{
        User = $userInfo[0]
        Password = $userInfo[1]
        Host = $uri.Host
        Port = if ($uri.Port -gt 0) { $uri.Port } else { 5432 }
        Database = $database
    }
}

$databaseUrl = Get-TomlValue -Path $configPath -Section "database" -Key "url"
$testDatabaseUrl = Get-TomlValue -Path $configPath -Section "database" -Key "test_url"
$databaseConfig = Parse-PostgresUrl -Url $databaseUrl
$testDatabaseConfig = Parse-PostgresUrl -Url $testDatabaseUrl

if (
    $databaseConfig.User -ne $testDatabaseConfig.User -or
    $databaseConfig.Password -ne $testDatabaseConfig.Password -or
    $databaseConfig.Host -ne $testDatabaseConfig.Host -or
    $databaseConfig.Port -ne $testDatabaseConfig.Port
) {
    throw "[database].url and [database].test_url must use the same host, port, username, and password"
}

$env:CRV_POSTGRES_USER = $databaseConfig.User
$env:CRV_POSTGRES_PASSWORD = $databaseConfig.Password
$env:CRV_POSTGRES_DB = $databaseConfig.Database
$env:CRV_POSTGRES_PORT = [string]$databaseConfig.Port
$env:CRV_POSTGRES_TEST_DB = $testDatabaseConfig.Database

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
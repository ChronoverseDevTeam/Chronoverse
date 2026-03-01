param(
    [string]$ImageName = "crv-hive:latest"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..")
$TargetTriple = "x86_64-unknown-linux-musl"
$BinaryPath = Join-Path $RepoRoot "target/$TargetTriple/release/crv-hive"

Write-Host "==> Repo root: $RepoRoot"
Write-Host "==> Build target: $TargetTriple"

Push-Location $RepoRoot
try {
    Write-Host "==> Ensuring rust target exists: $TargetTriple"
    rustup target add $TargetTriple

    Write-Host "==> Building release binary: crv-hive"
    cargo build -p crv-hive --release --target $TargetTriple

    if (-not (Test-Path $BinaryPath)) {
        throw "Release binary not found: $BinaryPath"
    }

    Write-Host "==> Building Docker image: $ImageName"
    docker build -f (Join-Path $ScriptDir "Dockerfile") -t $ImageName $RepoRoot

    Write-Host "==> Done. Image built: $ImageName"
}
finally {
    Pop-Location
}

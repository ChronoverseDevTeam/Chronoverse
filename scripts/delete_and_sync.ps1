$base = "http://127.0.0.1:3000/api/v1"
$ticket = "980ec03c-6f15-4cc0-9aa5-7d22d31a5cab:da259785-0986-427c-825a-ea605dfd070d:1781202737:c90478960dfe704de3dfa541dc95aba10d107e5ace4d536baea1a7961d8cc5a1"
$authHeader = @{Authorization="Ticket $ticket"}

Add-Type -AssemblyName System.Web

Write-Host "=== Count before delete ==="
$before = (Get-ChildItem D:\Test -Recurse -File | Measure-Object).Count
Write-Host "$before files"

Write-Host "`n=== Deleting all files ==="
Get-ChildItem D:\Test -Recurse -File | Remove-Item -Force
$after = (Get-ChildItem D:\Test -Recurse -File -ErrorAction SilentlyContinue | Measure-Object).Count
Write-Host "Remaining: $after"

Write-Host "`n=== Sync -f (get file list) ==="
$resp = Invoke-RestMethod -Uri "$base/clients/test/sync?force=true" -Method GET -Headers $authHeader
$files = $resp.data.files
Write-Host "Server says $($files.Count) files to restore"
Write-Host "Total bytes: $([math]::Round($resp.data.total_bytes/1MB, 2)) MB"

Write-Host "`n=== Downloading and writing files ==="
$i = 0; $errors = 0
foreach ($f in $files) {
    $i++
    try {
        # URL-encode the depot path (but keep / as-is)
        $encoded = [System.Web.HttpUtility]::UrlPathEncode($f.depot_path)
        $url = "$base/files/content/$encoded"
        $resp = Invoke-WebRequest -Uri $url -Method GET -Headers $authHeader -UseBasicParsing
        $localRel = $f.depot_path -replace '^//depot/Test/', ''
        $localPath = Join-Path "D:\Test" $localRel
        $dir = Split-Path $localPath -Parent
        if ($dir -and !(Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        [IO.File]::WriteAllBytes($localPath, $resp.Content)
    } catch {
        $errors++
        if ($errors -le 5) { Write-Host "  FAIL: $($f.depot_path) -- $_" }
    }
    if ($i % 20 -eq 0) { Write-Host "  $i/$($files.Count)" }
}
Write-Host "Downloaded: $i files, errors: $errors"

Write-Host "`n=== Verify ==="
$restored = (Get-ChildItem D:\Test -Recurse -File | Measure-Object).Count
Write-Host "Files on disk: $restored"
if ($restored -eq $before) {
    Write-Host "`n*** SUCCESS: All $restored files restored! ***"
} else {
    Write-Host "`n*** WARNING: $restored/$before restored ***"
}

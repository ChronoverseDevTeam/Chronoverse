$base = "http://127.0.0.1:3000/api/v1"
$ErrorActionPreference = "Stop"
$script:ticket = $null

function Post($path, $body) {
    $headers = @{"Content-Type"="application/json"}
    if ($script:ticket) { $headers["Authorization"]="Ticket $script:ticket" }
    $json = if ($body -is [string]) { $body } else { ConvertTo-Json $body -Compress -Depth 10 }
    $bytes = [Text.Encoding]::UTF8.GetBytes($json)
    $resp = Invoke-RestMethod -Uri "$base$path" -Method POST -Headers $headers -Body $bytes
    return $resp
}

function PostBinary($path, $filePath) {
    $headers = @{}
    if ($script:ticket) { $headers["Authorization"]="Ticket $script:ticket" }
    $bytes = [IO.File]::ReadAllBytes($filePath)
    return Invoke-RestMethod -Uri "$base$path" -Method POST -Headers $headers -Body $bytes -ContentType "application/octet-stream"
}

function Get-Api($path) {
    $headers = @{}
    if ($script:ticket) { $headers["Authorization"]="Ticket $script:ticket" }
    return Invoke-RestMethod -Uri "$base$path" -Method GET -Headers $headers
}

# 1. Bootstrap
Write-Host "=== Bootstrap ==="
try { Post "/bootstrap" "{}" | Out-Null } catch { Write-Host "  (already done)" }
Write-Host "OK"

# 2. Login
Write-Host "=== Login ==="
$resp = Post "/auth/login" @{user="admin"; password="admin123"}
$script:ticket = $resp.data.ticket
Write-Host "OK"

# 3. Create client 'test'
Write-Host "=== Create Client ==="
try { Post "/clients" @{name="test"; root="D:\\Test"} | Out-Null } catch { Write-Host "  (already exists)" }
Write-Host "OK"

# 4. Collect files
Write-Host "=== Collect Files ==="
$files = Get-ChildItem D:\Test -Recurse -File
$depotPaths = [System.Collections.ArrayList]@()
$fileMap = @{}
foreach ($f in $files) {
    $rel = $f.FullName.Substring(9) -replace '\\','/'   # D:\Test\ = 9 chars
    $dp = "//depot/Test/$rel"
    [void]$depotPaths.Add($dp)
    $fileMap[$dp] = $f.FullName
}
Write-Host "$($depotPaths.Count) files"

# 5. Open for add
Write-Host "=== Open for Add ==="
Post "/clients/test/files/add" @{files=$depotPaths.ToArray()} | Out-Null
Write-Host "OK"

# 6. Upload
Write-Host "=== Upload ==="
$total = $depotPaths.Count; $i = 0
foreach ($dp in $depotPaths) {
    $i++
    try { PostBinary "/files/content/$dp" $fileMap[$dp] | Out-Null }
    catch { Write-Host "FAIL $dp" }
    if ($i % 20 -eq 0) { Write-Host "$i/$total" }
}
Write-Host "$total OK"

# 7. Submit
Write-Host "=== Submit ==="
$resp = Post "/clients/test/changes" @{description="init"}
$cid = $resp.data.id
$resp = Post "/clients/test/changes/$cid/submit" @{}
Write-Host "change $($resp.data.change_number), $($resp.data.files_submitted) files"

Write-Host "`n=== DONE - Ticket: $script:ticket ==="

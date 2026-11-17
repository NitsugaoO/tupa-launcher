
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$appDir = Join-Path $env:APPDATA 'mihomo-party'
$profileYaml = Join-Path $appDir 'profile.yaml'
if (-not (Test-Path -LiteralPath $profileYaml)) { throw 'mihomo-party profile.yaml not found' }

$current = ''
$items = @{}
$id = ''
foreach ($line in Get-Content -LiteralPath $profileYaml -Encoding UTF8) {
    if ($line -match '^\s*-\s*id:\s*(\S+)') { $id = $matches[1]; $items[$id] = @{ name = ''; url = ''; updated = 0 } }
    elseif ($id -and $line -match '^\s+name:\s*(.+)$') { $items[$id].name = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+url:\s*(\S+)') { $items[$id].url = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+updated:\s*(\d+)') { $items[$id].updated = [int64]$matches[1] }
    elseif ($line -match '^current:\s*(\S+)') { $current = $matches[1] }
}
if (-not $current -or -not $items.ContainsKey($current)) { throw 'no current profile' }
$item = $items[$current]
if (-not $item.url) { throw '当前订阅没有 URL（本地配置无法在线更新）' }

$response = Invoke-WebRequest -Uri $item.url -UseBasicParsing -TimeoutSec 30 -Headers @{
    'User-Agent' = 'clash.meta/mihomo-party'
}
$content = $response.Content
if ($content -notmatch '(?m)^(proxies|proxy-providers):') {
    throw '下载到的内容不像 Clash 配置（缺少 proxies / proxy-providers）'
}

$profileFile = Join-Path $appDir ("profiles\$current.yaml")
if (Test-Path -LiteralPath $profileFile) { Copy-Item -LiteralPath $profileFile -Destination "$profileFile.bak" -Force }
[IO.File]::WriteAllText($profileFile, $content, (New-Object Text.UTF8Encoding($false)))

$lines = Get-Content -LiteralPath $profileYaml -Encoding UTF8
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$inCurrent = $false
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^\s*-\s*id:\s*(\S+)') {
        $inCurrent = ($matches[1] -eq $current)
    } elseif ($inCurrent -and $lines[$i] -match '^(\s*)updated:') {
        $lines[$i] = $matches[1] + 'updated: ' + $now
        $inCurrent = $false
    }
}
[IO.File]::WriteAllLines($profileYaml, $lines, (New-Object Text.UTF8Encoding($false)))

$exe = $null
try {
    $command = (Get-ItemProperty 'HKCU:\Software\Classes\clash\shell\open\command' -ErrorAction Stop).'(default)'
    $exe = ($command -split '"')[1]
} catch {}
if (-not $exe -or -not (Test-Path -LiteralPath $exe)) {
    $exe = 'C:\Program Files\Clash Party\Clash Party.exe'
}
Get-Process -Name 'Clash Party' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
Start-Process -FilePath $exe
Start-Sleep -Seconds 2
Write-Output '已更新并重启 Clash Party'

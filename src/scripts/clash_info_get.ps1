
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$appDir = Join-Path $env:APPDATA 'mihomo-party'
$profileYaml = Join-Path $appDir 'profile.yaml'
if (-not (Test-Path -LiteralPath $profileYaml)) { throw 'mihomo-party profile.yaml not found' }
$current = ''
$items = @{}
$id = ''
foreach ($line in Get-Content -LiteralPath $profileYaml -Encoding UTF8) {
    if ($line -match '^\s*-\s*id:\s*(\S+)') { $id = $matches[1]; $items[$id] = @{ name = ''; updated = 0 } }
    elseif ($id -and $line -match '^\s+name:\s*(.+)$') { $items[$id].name = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+updated:\s*(\d+)') { $items[$id].updated = [int64]$matches[1] }
    elseif ($line -match '^current:\s*(\S+)') { $current = $matches[1] }
}
if (-not $current -or -not $items.ContainsKey($current)) { throw 'no current profile' }
$item = $items[$current]
$name = if ($item.name) { $item.name } else { $current }
if ($item.updated -le 0) { Write-Output "$name · 从未更新"; exit 0 }
$span = (Get-Date) - [DateTimeOffset]::FromUnixTimeMilliseconds($item.updated).LocalDateTime
$age = if ($span.TotalMinutes -lt 1) { '刚刚' }
    elseif ($span.TotalHours -lt 1) { '{0:N0} 分钟前' -f $span.TotalMinutes }
    elseif ($span.TotalDays -lt 1) { '{0:N0} 小时前' -f $span.TotalHours }
    else { '{0:N0} 天前' -f $span.TotalDays }
Write-Output "$name · $age"

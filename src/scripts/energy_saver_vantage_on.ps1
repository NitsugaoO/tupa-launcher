
$mode = $null
try { $mode = Read-VantageMode } catch {}
if ($mode -and $mode -ne 'MmcCool') { Write-VantageMode 'MmcCool' }

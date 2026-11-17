
$mode = $null
try { $mode = Read-VantageMode } catch {}
if ($mode -and $mode -ne 'ItsAuto') { Write-VantageMode 'ItsAuto' }

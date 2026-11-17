
$state = 0
try { $state = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -Name EnergySaverState).EnergySaverState } catch {}
$mode = $null
try { $mode = Read-VantageMode } catch {}
$vantageOk = (-not $mode) -or ($mode -eq 'MmcCool')
if ($vantageOk -and ($state -eq 1)) { 'on' } else { 'off' }


$v = [byte][int]$env:TUPA_VALUE
$m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Select-Object -First 1
if (-not $m) { exit 3 }
$null = Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{ Timeout = [uint32]1; Brightness = $v } -ErrorAction Stop

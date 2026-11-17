
$b = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1
if (-not $b) { exit 3 }
$b.CurrentBrightness

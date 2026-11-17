
$ErrorActionPreference = 'Stop'
Get-Process nosleep -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
if (Get-Process nosleep -ErrorAction SilentlyContinue) { throw 'nosleep is still running' }
'off'

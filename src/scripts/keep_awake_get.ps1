
$ErrorActionPreference = 'Stop'
if (Get-Process nosleep -ErrorAction SilentlyContinue) { 'on' } else { 'off' }

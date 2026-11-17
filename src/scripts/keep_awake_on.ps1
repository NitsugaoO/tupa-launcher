
$ErrorActionPreference = 'Stop'
if (-not (Get-Process nosleep -ErrorAction SilentlyContinue)) {
    Start-Process 'D:\Softwares\nosleep\nosleep.exe' -ArgumentList '--tray','--startup'
    Start-Sleep -Milliseconds 800
}
if (-not (Get-Process nosleep -ErrorAction SilentlyContinue)) { throw 'nosleep did not start' }
'on'

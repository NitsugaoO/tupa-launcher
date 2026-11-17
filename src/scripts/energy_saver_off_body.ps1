
$want = 2
if ((Get-State) -eq $want) { 'off'; exit 0 }
if (-not (Is-Open)) { [TupaEnergy]::WinA(); if (-not (Wait-Open)) { throw 'quick settings did not open' } }
try {
    [TupaEnergy]::SaveCursor()
    Start-Sleep -Milliseconds 150
    Click-Energy
    if (-not (Wait-State $want)) { throw 'energy saver did not change (quick settings layout changed?)' }
} finally {
    [TupaEnergy]::RestoreCursor()
    if (Is-Open) { [TupaEnergy]::WinA() }
}
'off'

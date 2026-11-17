
$ErrorActionPreference = 'Stop'
$vantageRoot = Join-Path $env:ProgramData 'Lenovo\Vantage\Addins\IdeaNotebookAddin'
$vantageDir = $null
$vantageType = $null
$vantageAgent = $null
try {
    $vantageDir = Get-ChildItem -LiteralPath $vantageRoot -Directory -ErrorAction Stop |
        Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
        Select-Object -First 1
} catch { $vantageDir = $null }
if ($vantageDir) {
    foreach ($name in 'Newtonsoft.Json.dll','Lenovo.Vantage.RpcCommon.dll','Lenovo.Vantage.RpcClient.dll','Lenovo.VantageService.Utilities.dll','Lenovo.VantageService.UtilitiesExtensions.dll','PowerContract.dll') {
        $path = Join-Path $vantageDir.FullName $name
        if (Test-Path -LiteralPath $path) { try { [Reflection.Assembly]::LoadFrom($path) | Out-Null } catch {} }
    }
    $vantageAssembly = [Reflection.Assembly]::LoadFrom((Join-Path $vantageDir.FullName 'IdeaPowerAgent.dll'))
    $vantageType = $vantageAssembly.GetType('IdeaNotebookAddin.PowerAgent')
    $vantageAgent = $vantageType.GetMethod('GetInstance', [Reflection.BindingFlags]'Public,Static,Instance').Invoke($null, @())
}
function Read-VantageMode {
    if (-not $vantageType) { return $null }
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $response = $vantageType.GetMethod('GetITSMode').Invoke($vantageAgent, @($request))
    return [string]$response.ItsMode
}
function Write-VantageMode([string]$mode) {
    if (-not $vantageType) { return }
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $request.ItsMode = [Lenovo.Modern.Contracts.Power.ItsModeType]$mode
    $null = $vantageType.GetMethod('SetITSMode').Invoke($vantageAgent, @($request))
}

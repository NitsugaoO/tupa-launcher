
$ErrorActionPreference = 'Stop'
$root = Join-Path $env:ProgramData 'Lenovo\Vantage\Addins\IdeaNotebookAddin'
$dir = Get-ChildItem -LiteralPath $root -Directory -ErrorAction Stop |
    Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
    Select-Object -First 1
if (-not $dir) { throw 'IdeaNotebookAddin not installed' }
$battery = Join-Path $dir.FullName 'IdeaBatteryAgent.dll'
if (-not (Test-Path -LiteralPath $battery)) { throw 'IdeaBatteryAgent.dll not found' }
foreach ($name in 'Newtonsoft.Json.dll','Lenovo.Vantage.RpcCommon.dll','Lenovo.Vantage.RpcClient.dll','Lenovo.VantageService.Utilities.dll','Lenovo.VantageService.UtilitiesExtensions.dll','BatteryManagementContract.dll','BatteryInformationContract.dll','PowerContract.dll') {
    $path = Join-Path $dir.FullName $name
    if (Test-Path -LiteralPath $path) { try { [Reflection.Assembly]::LoadFrom($path) | Out-Null } catch {} }
}
$assembly = [Reflection.Assembly]::LoadFrom($battery)
$type = $assembly.GetType('IdeaNotebookAddin.BatteryAgent')
$agent = $type.GetMethod('GetInstance', [Reflection.BindingFlags]'Public,Static,Instance').Invoke($null, @())


$request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.BatteryManagement.BatteryMgmtRequest])
$request.BatteryChargeMode = [Lenovo.Modern.Contracts.BatteryManagement.BatteryChargeModeType]::Normal
$response = $type.GetMethod('SetBatteryChargeMode').Invoke($agent, @($request))
$response.BatteryChargeMode

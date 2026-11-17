
$response = $type.GetMethod('GetBatteryChargeMode').Invoke($agent, @())
$response.BatteryChargeMode

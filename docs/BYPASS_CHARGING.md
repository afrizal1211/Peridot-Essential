# Peridot charging and bypass

The adaptation preserves OEM charging and available battery/charger telemetry.
Charging power profiles are not calibrated for the POCO F7 and are rejected as
unavailable. Current values from other hardware are not reused.

Bypass support requires the explicit capability and state nodes checked by the
backend. They were absent from the supplied snapshot, so
`bypass_charging_supported=0` is expected.

Neither input_suspend nor charge_control_limit is treated as a validated bypass
control. The tile and UI cannot create missing kernel support. Further adaptation
requires device nodes, readback and physical tests; fixed charging power is not
promised.

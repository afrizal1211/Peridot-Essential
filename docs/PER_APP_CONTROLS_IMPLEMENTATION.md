# Per-app controls

The daemon manages application profiles and control ownership. The UI process
does not need to remain in the foreground. Configuration follows detected
capabilities and does not add missing hardware nodes.

CPU uses policies 0/2/5/7 and real kernel frequency tables. GPU uses KGSL profiles.
Driver and vendor constraints remain visible as effective bounds that may differ
from requests.

Repeated events for the same application should not continuously rewrite the
hardware. Leaving a profile restores global preferences or captured original
values according to the control path.

Sconfig is an independent global selection in CPU, not a per-game setting.
Fixed-rate touch controls have not been validated for this adaptation. Xiaomi
scenario or application changes can alter limits; inspect effective readback
after applying a profile.

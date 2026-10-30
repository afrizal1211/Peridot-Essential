# Peridot port maintainer guide

This adaptation targets POCO F6 peridot with Xiaomi vendor interfaces and display
services. A system port from another brand does not itself change the physical
vendor interfaces.

Native integration remains pending. Review [ROM_INTEGRATION.md](ROM_INTEGRATION.md)
before using exporters. Collect actual contexts and services, validate app/daemon
identity and test controls with SELinux enforcing.

Exported files are not a validated flashable module. For the root module route,
use [BUILDING.md](BUILDING.md) and the prepared workflow.

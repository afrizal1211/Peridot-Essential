# Peridot integration into an unpacked ROM

See [ROM_INTEGRATION.md](ROM_INTEGRATION.md). Native integration and SELinux
validation remain pending for this variant; the root module is the prepared route.
An exporter does not validate policies or vendor compatibility.

Use the APK and daemon from the same peridot.4 build, verify actual device permissions
and contexts, and avoid multiple instances controlling the hardware.

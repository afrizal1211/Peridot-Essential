# Testing on a stock ROM

Peridot uses a regular app and a daemon supplied by the root module. Build and
install the complete module, reboot and open `io.github.davvv98.peridotessential`.

First check protocol=peridot.4 and communication through peridot_ctl. Then test one
feature at a time, comparing requests with effective readback. A successful
sysfs write may still be constrained by the driver or vendor.

Unpacked-ROM modification tools do not replace device testing. Native integration
and SELinux validation remain pending; see [ROM_INTEGRATION.md](ROM_INTEGRATION.md).

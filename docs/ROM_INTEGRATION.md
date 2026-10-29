# Native Peridot ROM integration

The prepared route covered by contract checks is the KernelSU Next/Magisk module.
Native ROM integration and included SELinux policies **have not been validated
for this variant**. Matching project names do not establish matching vendor nodes.

Current identity: package `io.github.davvv98.peridotessential`, activity
`PeridotActivity`, binaries `peridot_daemon` and `peridot_ctl`, host library
`libperidot_essential_host.so`, protocol `peridot.4`.

Integration requires actual KGSL, cpufreq, Sconfig and Xiaomi service contexts,
process domains and tests with SELinux enforcing. Export scripts are maintainer
tools, not a boot guarantee or a ready-to-install AOSP bundle for this port.

Do not run a native backend and a module daemon controlling the same hardware
simultaneously. ROM policy must retain transport authentication and app isolation.

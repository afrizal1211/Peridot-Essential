# Peridot Essential

Hardware control application for **POCO F6 (`peridot`)**, Snapdragon 8s Gen 3
(`SM8635`), Adreno 735 GPU and Android ARM64. It includes a Flutter interface,
a Rust host and a separate privileged daemon provided by the root module.

<img src="branding/peridot-icon-master.png" alt="Peridot Essential logo" width="128" />

## Community build

This build is distributed by **afrizal1221** for the
[POCO F6 Indonesia](https://t.me/PocoF6Indo) community.

- Repository: [Peridot-Essential](https://github.com/afrizal1211/Peridot-Essential).
- Telegram: [@PocoF6Indo](https://t.me/PocoF6Indo).
- Community changes: signed release builds, release automation, artifact naming
  and the community identity shown in the app. Hardware behaviour is unchanged
  from the adapted source credited below.

## Credits and origin

**Peridot Essential is an adaptation of Rodin Essential, created by
[NEESCHAL-3](https://github.com/NEESCHAL-3) and contributors.**
The Peridot name and POCO F6 support do not replace the original authorship.

- Original project: [Rodin Essential](https://github.com/NEESCHAL-3/Rodin-Essential).
- Fork: [Peridot-Essential](https://github.com/davvv98/Peridot-Essential).
- Peridot / POCO F6 adaptation maintained by **davvv98**.
- Adaptation changes: KGSL backend, F6 CPU policies, Sconfig selector, Peridot
  identity, icons, documentation and build adjustments.

The inherited architecture and features remain the work of the original author
and contributors. See [CREDITS.md](CREDITS.md), [NOTICE](NOTICE) and [LICENSE](LICENSE).

## Identity

| Component | Identifier |
| --- | --- |
| Application | Peridot Essential |
| Package | `io.github.davvv98.peridotessential` |
| Activity | `PeridotActivity` |
| Daemon | `peridot_daemon` |
| Diagnostic client | `peridot_ctl` |
| Host library | `libperidot_essential_host.so` |
| Protocol | `peridot.5` |
| Internal version | `1.18.8` / `11808` | |
| Community maintainer | afrizal1221 |
| Community chat | [@PocoF6Indo](https://t.me/PocoF6Indo) |

## Adaptation features

- CPU policies `0`, `2`, `5`, `7`, governors and ranges validated against the
  kernel frequency tables. Requests resolve to exact kHz values.
- Qualcomm KGSL GPU profiles: Stock, Gaming, Saver and Extreme. Observed
  frequency table: **264–1150 MHz**, governor `msm-adreno-tz`.
- Explicit Xiaomi Sconfig selection in CPU, independent of frequency requests
  and GPU profiles, with original-value restoration.
- Separate saved requests and effective limits. An accepted write with different
  effective bounds is not reported as an exact frequency match.
- Inherited per-app controls, telemetry, ZRAM and storage features depend on
  the nodes actually available on the device.
- Red/gold Peridot logo and adaptive icon; Sconfig cards grouped into
  Standard/Performance with Normal, Game and NoLimits modes.

## Status and known limitations

On-device checks on POCO F6 passed for thermal control, CPU frequency and
efficiency including prime, manual core control, GPU performance mode, governor
efficiency and prime, per-app controls, display and resolution. The items below
are the remaining gaps.

Device testing confirmed 1150 MHz GPU operation with Sconfig scenario 6.
This does not guarantee every frequency in every scenario. Driver and vendor
services can change effective limits; the backend does not continuously rewrite
the requested frequencies to fight those changes.

Touch control is **unavailable on the tested peridot vendor stack**: the vendor
TouchFeature AIDL service is not exposed, so OEM (about 125 Hz) and game mode
(about 250 Hz) remain inherited labels rather than working profiles here. The
app detects this and writes no touch mode. The 480 Hz option stays disabled and
the 1000 Hz resampling option was removed. Charging power profiles **still
require adaptation** and are kernel dependent. OEM charging and available
telemetry remain enabled. Bypass is available only when the explicit nodes
required by the backend are detected; they were absent from the supplied device
snapshot. Memory DVFS and UFS scheduler controls have **not been device-validated**
yet; their nodes are probed at runtime and reported as unavailable when absent.

Xiaomi DisplayFeature AIDL is retained, but an accepted transaction does not
prove that every display effect works on the port. Native ROM integration and
SELinux policies have not been validated for this variant.

## Build and installation

The prepared route is **GitHub Actions → peridot-release → Run workflow**.
See [GITHUB_ACTIONS.md](GITHUB_ACTIONS.md) for signing and artifacts, or
[docs/BUILDING.md](docs/BUILDING.md) for local builds.

Install the complete module and reboot. The APK and daemon must both use
`peridot.5`. The new application package has separate Android data and does not
update the previous package. The internal root module identifier is retained
for upgrade compatibility; its processes and executables use Peridot names.

## Guides

- [Sconfig and OEM controls](docs/OEM_CONTROL.md)
- [CPU and GPU diagnostics](docs/ARCHITECTURE.md)
- [Per-app controls](docs/PER_APP_CONTROLS_IMPLEMENTATION.md)
- [Flutter engine and APK size](docs/FLUTTER_RUNTIME.md)
- [System colors](docs/SYSTEM_COLORS.md)
- [Charging and bypass](docs/BYPASS_CHARGING.md)
- [ROM integration: pending validation](docs/ROM_INTEGRATION.md)
- [Changelog](CHANGELOG.md)
- [Branding and icons](branding/README.md)

## Validation

This revision has passed 107 daemon unit tests and the daemon integration suite,
Android ARM64 Rust checks for the host and daemon, contract checks, Dart syntax,
XML and package/JNI consistency checks. The latest identity and Sconfig panel
still require a full APK build and visual validation on the device.

## License

Apache-2.0. [LICENSE](LICENSE) and source copyright notices are preserved.

# Peridot Essential changelog

## 1.18.8

Peridot (`POCO F6`, SM8635) hardware interface release. Protocol bumped to
`peridot.5`; version 1.18.8 / 11808.

- Memory DVFS now targets the SM8635 DCVS boost floors instead of the retired
  per-device devfreq rails. `subsystem.clocks.boost <rail> <hz>` writes a
  `boost_freq` floor vote on the ddr/l3/llcc/ddrqos rails under
  `/sys/devices/system/cpu/bus_dcvs`; `subsystem.clocks.trial memory <rail>`
  runs a 30 s journaled trial. Legacy devfreq memory journals are dropped on
  load because they never referenced a valid node.
- Storage targets the `1d84000.ufshc` devfreq (accepts both `ufshc` and
  `ufshci` names); UFS scheduler probing reads it from the hardware contract.
- Charging writes `fastcharge_enable` (0 none / 1 fast / 2 super fast) at
  `/sys/class/qcom-battery/fastcharge_enable`. The UI now offers OEM (0), Fast
  (33) and Super Fast (90) profiles; legacy 25/33/65/85/90/8 storage values are
  normalized onto that domain.
- Touch falls back to the LineageOS HTPR boolean (`bump_sample_rate`) when the
  Xiaomi TouchFeature AIDL service is absent; the chosen interface is exposed
  as `touchHal`/`touchBackend` and restored on release.
- New read-only probe `GET hardware.contract` reports DCVS rails, UFS devfreq,
  fastcharge node and touch backend for on-device validation.

## 1.18.7

Documentation and in-app copy accuracy for the community build. Protocol stays
`peridot.4`; no control path, frequency table or OPP limit changed.

- Touch Response no longer claims a live 125/250 Hz result on this device. The
  tested peridot vendor stack does not expose the TouchFeature AIDL service, so
  the screen now says the engine is unavailable, disables the profile grid and
  writes no touch mode. The rates remain inherited labels.
- README and `docs/OEM_CONTROL.md` match the device run: touch control
  unavailable, charging profiles kernel dependent, memory DVFS and UFS still
  unvalidated. The UFS/DVFS screen keeps probing nodes and reports them as
  unavailable when absent.

## 1.18.6

Community build for POCO F6 (`peridot`) by afrizal1221 / @PocoF6Indo.
Protocol stays `peridot.4`; hardware behaviour is unchanged.

- Community identity: the About page GitHub card and footer credit name
  afrizal1221, the Telegram card and the opened link point to @PocoF6Indo,
  and a tappable credit line keeps Rodin Essential (NEESCHAL-3) and the
  Peridot adaptation (davvv98) reachable from inside the app.
- Correct the documented hardware: POCO F6, Snapdragon 8s Gen 3 (`SM8635`),
  Adreno 735. Earlier text claimed POCO F7 / `SM8735` / Adreno 825, and one
  About hero line named MT6899, which is a MediaTek part number.
- Community distribution and release automation are credited in README,
  CREDITS, NOTICE and the module author field without changing the original
  authorship text.

## 1.18.5

POCO F6 adaptation revision; current protocol: `peridot.4`. This log describes
the delivered source and does not imply that a release has already been published.

- Disable implicit Rust binary discovery: only peridot_daemon/peridot_ctl are built;
  remove explicitly renamed legacy files before compilation.
- Replace touch choices with OEM (125 Hz observed), game mode (250 Hz observed)
  and disabled 480 Hz pending adaptation. Remove the 1000 Hz option.
- Enter expert color mode through feature 26, match the vendor Binder flag,
  validate expert channel ranges and verify calibration through decoded HAL dumps.
- Add Qualcomm KGSL telemetry and profiles based on real OPPs.
- Support CPU policies 0/2/5/7 with exact frequency conversion to kHz.
- Separate saved requests from effective bounds; retain IO and invalid-read errors.
- Remove periodic frequency rewrites triggered by drift.
- Add Xiaomi Sconfig Normal, Game, NoLimits and Performance variants, including
  original-value capture, restoration, persistence and live readback.
- Use package `io.github.davvv98.peridotessential`, `PeridotActivity`,
  `peridot_daemon`/`peridot_ctl`, Peridot JNI and native APIs.
- Add the Peridot logo, launcher/adaptive icons and grouped Sconfig cards.
- Update GitHub Actions, PERIDOT_* signing variables and Flutter engine symbol
  stripping before APK packaging.
- Update documentation for Peridot hardware and current implementation status.

### Pending adaptation or validation

- Fixed-rate touch and resampling, charging power profiles and bypass.
- Xiaomi DisplayFeature effects on the device port.
- Native ROM integration and SELinux policies.
- Final APK build and visual validation of the new identity and Sconfig panel.

## Origin

Derived from Rodin Essential by NEESCHAL-3 and contributors. This log describes
Peridot changes and does not replace upstream history or authorship.
See [CREDITS.md](CREDITS.md).

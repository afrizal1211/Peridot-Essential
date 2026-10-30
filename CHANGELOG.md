# Peridot Essential changelog

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

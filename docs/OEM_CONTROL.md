# Sconfig and OEM controls

## Xiaomi Sconfig

The Thermal policy card in CPU explicitly selects the Xiaomi scenario.
It writes `/sys/class/thermal/thermal_message/sconfig`, falling back to
`/sys/devices/virtual/thermal/thermal_message/sconfig`. It does not modify ODM XML.

| Standard | Performance | Stock XML name |
| --- | --- | --- |
| 0 | 50 | Normal / per-normal |
| 18 | 68 | Game / per-game |
| 6 | 56 | NoLimits / per-noLimits |

Switching groups only changes the displayed choices. Tapping a card submits its
scenario; ACTIVE requires matching live readback. Names come from the supplied
stock XML. Device testing confirmed that mode 6 allowed 1150 MHz GPU operation;
identical behavior is not assumed for mode 56. XML `defined` fields describe
scenario conditions and must not be written as complete strings to sysfs.

Sconfig affects CPU and GPU policy. NoLimits can increase temperature and power
consumption. Frequency or GPU profile changes do not automatically select it.

## Persistence and restoration

- Capture the original value before the first change and verify each write.
- OEM restores that value and clears the saved selection.
- Disabling control restores the original while retaining the selection for
  reactivation. Reset restores the original and clears the selection.
- Enabling control or booting may reapply a saved selection.
- Uninstall attempts restoration over RPC before stopping the daemon; this
  requires a responsive client and daemon.

If a manual command already selected 6, the captured original is 6. Select 0
explicitly to request Normal. An original value of -1 is preserved literally.
There is no rewrite loop. Vendor changes after switching scenarios may require
resubmitting the desired CPU or GPU range.

Snapshot fields: `sconfig_supported`, `sconfig_live`, `sconfig_saved`,
`sconfig_original`. Saved -1 means OEM; original -2 means no capture yet.
RPC: `SET cpu.sconfig 6`; restoration: `SET cpu.sconfig -1`.

## Display and touch

The display backend uses Xiaomi DisplayFeature AIDL. Individual effects require
validation on the port; an accepted call does not guarantee a visible change.
Touch profiles are defined as OEM (mode 0 = 0; about 125 Hz label) and 250 Hz
(mode 0 = 1; about 250 Hz label). Both use TouchFeature AIDL transaction 9 and
verify game-mode readback through transaction 1. These labels are inherited
device observations, not daemon measurements of physical sampling rate.

On the tested peridot vendor stack the TouchFeature AIDL service is not
exposed. The app reports the engine as unavailable and writes no touch mode, so
the profile labels are informational there. The rates only become actionable on
a stack that exposes `vendor.xiaomi.hw.touchfeature.ITouchFeature`.

The original game-mode value is journaled before changing it and restored when
control is disabled or released. OEM explicitly switches game mode off. Profile
selection persists across reboot. Old pending/resampled selections are discarded.
480 Hz remains visible but disabled; the 1000 Hz option has been removed.
Requests for pending or removed profiles are rejected by the daemon, including
per-app requests. No THP memory modification or event injection is enabled by
these two profiles.

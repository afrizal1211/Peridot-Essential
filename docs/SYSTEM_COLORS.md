# System colors and display

These use separate control paths:

- Material You colors are queried/applied through SettingsProvider and system
  capabilities. Results depend on the ROM.
- Panel calibration uses the vendor Xiaomi DisplayFeature AIDL service.

A present service and an accepted call do not prove a visible color change.
Calibration IDs and effects require validation on the device port. Peridot does
not use Oplus Display for this path.

For diagnostics, attempt the change and inspect the snapshot and PeridotEssential
logs. Distinguish Android interface palette changes from panel image calibration.

## Peridot expert calibration

The device test confirmed a visible cooler tint with feature 26, gamut value 2
(cookie 0), followed by red value 200 (cookie 1). Original PRO now enters expert
mode using feature 26 instead of selecting the separate Standard color case.

The AIDL client uses transaction 7, four integers and the vendor transaction
flag. Expert activation, gamut and channel changes verify Enable and the requested
value against the Xiaomi diagnostic dump before saving the selection. The dump
uses MIDISP encoding with a stream-wide position, including newline positions.
Readback confirms HAL state, not physical color accuracy. HDR and other vendor
states can still prevent a request from taking effect; mismatches remain errors.

Expert ranges follow the supplied hardware implementation: RGB 50–255, hue
-180–180, saturation -50–50, value -255–255, contrast -100–100 and gamma 170–270.
Vivid, Saturated and temperature presets still require individual device tests;
the RGB test does not establish that every display effect works.

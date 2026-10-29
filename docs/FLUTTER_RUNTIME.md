# Peridot Flutter engine

Peridot uses a custom Flutter embedder rather than the ROM engine. The SDK is
pinned to Flutter 3.47.7. The engine revision comes from
`bin/internal/engine.version` and may differ from the SDK tag commit.

Actions builds the engine in a separate checkout, avoiding SDK changes during
gclient sync. The build script checks the revision before compiling.
`embedder.h`, the engine and the AOT snapshots must correspond.

Generated files are ignored by Git:

- `runtime/flutter-engine/prebuilt/android-arm64/libflutter_engine.so`
- `runtime/flutter-engine/prebuilt/android-arm64/icudtl.dat`

The Actions cache includes the header and revision marker. Before APK packaging,
`llvm-strip --strip-unneeded` removes engine symbols. Logs show sizes before and
after stripping. This removes debug content; final size must be measured from
the built APK.

The APK contains ARM64 code, an AOT interface and a small Java framework adapter
for the activity and tile. It is not a zero-DEX APK. Build scripts check native
symbols and 16 KB alignment. Final device validation includes startup, input,
screen transitions and returning from the background.

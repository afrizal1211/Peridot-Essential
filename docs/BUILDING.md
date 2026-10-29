# Building Peridot Essential

The prepared build route is [GitHub Actions](../GITHUB_ACTIONS.md). Local builds
require Linux x86_64, Flutter 3.47.7, Rust with `aarch64-linux-android`, JDK 17,
Android SDK API 36, build-tools 36.0.0, NDK 27.2.12479018, ninja, git, curl,
zip/unzip, file and readelf.

```sh
rustup target add aarch64-linux-android
cargo test --locked -p peridot-essential-daemon
cargo check --locked --target aarch64-linux-android -p peridot-essential-daemon -p peridot-essential-host
(cd ui/flutter && flutter pub get)
```

Build the embedder engine from the revision in `engine.version`, in a checkout
separate from the SDK. See [FLUTTER_RUNTIME.md](FLUTTER_RUNTIME.md).

```sh
PERIDOT_FLUTTER_ENGINE_SRC=/path/to/checkout/engine/src bash tools/build-flutter-engine.sh
PERIDOT_BUILD_ONLY=1 bash build-and-install.sh
```

Set `ANDROID_SDK_ROOT` to your SDK. Explicit versions can be selected through
`PERIDOT_NDK_VERSION`, `PERIDOT_BUILD_TOOLS_VERSION` and `PERIDOT_PLATFORM_VERSION`.
Without `PERIDOT_BUILD_ONLY=1`, the development script may install on a connected
ADB device.

## Signing and module packaging

`PERIDOT_KEYSTORE`, `PERIDOT_KEY_ALIAS`, `PERIDOT_KEYSTORE_PASS` and `PERIDOT_KEY_PASS`
configure signing. For a custom key, the packager accepts
`PERIDOT_EXPECTED_CERT_SHA256` and checks the APK certificate against that hash.
Otherwise it uses the expected certificate stored in the project.

```sh
bash tools/build-kernelsu-next-module.sh
```

This packager requires a keystore. Key generation and Actions secrets are
described in the Actions guide.

Outputs include `Peridot-Essential.apk`, `peridot_daemon`/`peridot_ctl` and the module ZIP.
Use the APK and module from the same build. Keep generated artifacts and signing
keys out of the repository.

## Removing stale files after copying an update

Copying a ZIP over a checkout can leave previously renamed files behind.
Before committing, run:

```sh
bash tools/cleanup-legacy-paths.sh
git add -A
```

Cleanup removes only explicitly listed renamed source paths when their Peridot
replacement exists. It does not remove credits, LICENSE, NOTICE or Git history.
The workflow also runs cleanup before compiling. The daemon crate sets
`autobins = false`, so Cargo builds only the declared `peridot_daemon` and
`peridot_ctl` binaries even if old entry points remain in src/bin.

Inherited dead_code warnings are not import failures and do not mean the daemon
failed to compile. They are not globally suppressed.

# Building Peridot with GitHub Actions

The workflow is `.github/workflows/peridot-release.yml`. Put the project contents
at the repository root, including `.github`; uploading only the source ZIP
does not unpack it into a runnable workflow.

## First build

1. Push the source to your branch.
2. Open Actions → peridot-release → Run workflow and select that branch.
3. Download the Peridot-Essential artifact after completion.

It contains `Peridot-Essential.apk`, `Peridot-Essential-KernelSU-Next-Magisk.zip`
and `SHA256SUMS.txt`. Manual runs without signing secrets use a temporary key;
APKs from different runs may not be compatible for in-place updates.

## Persistent signing

Configure these under Settings → Secrets and variables → Actions:

| Secret | Value |
| --- | --- |
| PERIDOT_KEYSTORE_BASE64 | Complete JKS encoded as base64 |
| PERIDOT_KEY_ALIAS | Key alias; default peridot-essential |
| PERIDOT_KEYSTORE_PASS | JKS password |
| PERIDOT_KEY_PASS | Key password; defaults to the JKS password |

Test defaults use android as the password. Use your own release key and keep
the JKS outside the repository.

```sh
keytool -genkeypair -storetype JKS -keystore peridot-release.jks \
  -alias peridot-essential -keyalg RSA -keysize 4096 -validity 9125 \
  -dname 'CN=Peridot Essential,O=Peridot Essential,C=MX'
base64 -w 0 peridot-release.jks > peridot-release-base64.txt
```

Copy the TXT contents into the secret; do not publish those files.
The workflow compares the APK signature against the configured key certificate.
A separate PAT or disabling signature verification is unnecessary.

## Tag releases

Tags matching v* trigger the workflow and publish artifacts. They require
PERIDOT_KEYSTORE_BASE64 to prevent releases signed with temporary keys.

```sh
git tag v1.18.5-peridot.4
git push origin v1.18.5-peridot.4
```

The tag does not automatically change the internal APK version.

## Tools and status

Flutter 3.47.7, API 36, build-tools 36.0.0 and NDK 27.2.12479018 are pinned.
The engine uses a separate checkout pinned to engine.version and is cached
with its header. Scripts honor explicit workflow versions and use PERIDOT_*
variables and the peridot-essential-daemon/peridot-essential-host crates.

The workflow produces the app and root module. Native ROM bundles remain
pending validation. Source and syntax checks do not replace the first complete
GitHub build or APK testing on the device.

# Android: signing and releases

The Android app lives in [`android/`](../android/) and releases on its own tag
namespace, `android-X.Y.Z`, independently of the Linux `vX.Y.Z` tags. This page
covers the one-time signing setup; the day-to-day release flow is in
[RELEASING.md](RELEASING.md).

## Versioning

There is no version file in `android/`. The tag is the single source of truth:
`release-android.yml` takes `versionName` from the tag (minus the `android-`
prefix) and derives `versionCode` as `major * 10000 + minor * 100 + patch`, then
passes both to Gradle as environment variables. A local build is always
`versionName = "dev"`, `versionCode = 1`, which no release can collide with.

That scheme stays monotonic as long as minor and patch stay below 100, which
matches the semver bumps `auto-release.yml` produces.

## One-time signing setup

Android requires every release APK to be signed with a key you keep. Losing it
means users cannot upgrade in place, so back it up somewhere durable.

1. Generate the keystore locally. **It must never be committed** - the repo's
   `.gitignore` blocks `*.keystore` and `*.jks` precisely so this cannot happen
   by accident:

   ```bash
   keytool -genkeypair -v \
     -keystore fodder.keystore \
     -alias fodder \
     -keyalg RSA -keysize 4096 -validity 10000
   ```

   Keep the alias as `fodder`: the workflow passes it explicitly, and it is not
   sensitive.

2. Add three repository secrets (Settings -> Secrets and variables -> Actions):

   | Secret | Value |
   |--------|-------|
   | `ANDROID_KEYSTORE_BASE64` | `base64 -w 0 fodder.keystore` |
   | `ANDROID_KEYSTORE_PASSWORD` | the keystore (store) password |
   | `ANDROID_KEY_PASSWORD` | the key password |

3. Store the keystore file and both passwords in your password manager.

The workflow decodes the keystore to `android/fodder.keystore` for the build and
deletes it afterwards. Without `ANDROID_KEYSTORE_BASE64` set, the release job
fails early with a pointer back to this page rather than shipping an unsigned or
debug-signed APK.

A local `assembleRelease` has no keystore, so it falls back to the debug signing
config. That is fine for a smoke test and useless for distribution - only CI
produces a real release APK.

## Cutting a release

Two ways, both ending in a GitHub Release with the APK attached:

- **Automatic.** Merge a PR that touches `android/`. `auto-release.yml` computes
  the next `android-*` tag from the newest one and the PR's `release:*` label
  (default patch), pushes the tag, and calls `release-android.yml`.
- **Manual**, for an off-cycle build:

  ```bash
  git tag android-0.2.0 && git push origin android-0.2.0
  ```

A merge that touches only `linux/` cuts a Linux release and no APK, and vice
versa; a merge touching both cuts both.

## Local development

```bash
cd android
./gradlew testDebugUnitTest    # JVM unit tests (Robolectric for the DAO tests)
./gradlew ktlintCheck          # lint; ktlintFormat fixes most findings
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

To exercise the background poll without waiting for WorkManager's window:

```bash
adb shell dumpsys jobscheduler | grep -A5 io.github.dipakmdhrm.fodder
adb shell cmd jobscheduler run -f io.github.dipakmdhrm.fodder <jobId>
```

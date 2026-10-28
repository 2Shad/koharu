<h1 align="center">
  <img src="crates/koharu/icons/icon.png" width="96" alt="Koharu icon">
  <br>
  Koharu Android
</h1>

<p align="center">On-device ML manga translation for Android.</p>

<p align="center">
  <a href="https://github.com/2Shad/koharu/releases"><img alt="Android alpha release" src="https://img.shields.io/github/v/release/2Shad/koharu?include_prereleases&label=Android%20alpha&style=for-the-badge"></a>
  <img alt="Android 12+" src="https://img.shields.io/badge/Android-12%2B-3DDC84?logo=android&logoColor=white&style=for-the-badge">
  <img alt="arm64-v8a" src="https://img.shields.io/badge/arch-arm64--v8a-blue?style=for-the-badge">
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="Android_build_plan.md">Build plan</a> ·
  <a href="https://github.com/koharu-rs/koharu">Desktop app</a>
</p>

**Koharu Android** is the Android port of [Koharu](https://github.com/koharu-rs/koharu), an ML-powered manga translator. It runs the translation pipeline directly on the phone: text detection, OCR, translation, inpainting, and typesetting. The desktop application lives on [`main`](https://github.com/2Shad/koharu/tree/main) and in the upstream [koharu-rs/koharu](https://github.com/koharu-rs/koharu) repository.

> [!NOTE]
> This is an alpha port. The desktop app remains the reference implementation, and Android-specific behavior may still differ from it.

## What works

- Full on-device pipeline: detection → OCR → translation → inpainting → typesetting.
- Local translation through llama.cpp, with quantized model profiles selected from the device's RAM class.
- Local detection and OCR, including MangaOCR and PaddleOCR-VL profiles.
- AOT inpainting on device.
- Multi-page projects, a touch-first canvas, page swiping, and hold-to-reorder pages.
- Background processing through an Android foreground service.
- The same project format and Rust pipeline as the desktop app.

Inference is local by default. Downloaded models stay on the device; there is no companion desktop process or remote inference server.

## Requirements

- Android 12 or newer (API 31)
- `arm64-v8a`
- 8 GB of RAM or more recommended. Devices with 6–8 GB use the smaller translation profile.
- Several GB of free storage for the app, its native runtimes, and models.

The APK is large because the llama.cpp and libtorch runtimes are bundled rather than downloaded separately.

## Install

Download the latest Android alpha or preview from the [releases page](https://github.com/2Shad/koharu/releases), then install it:

```sh
adb install -r app-universal-release.apk
```

You can also copy the APK to the phone and open it. Releases are currently signed with the Android debug key, so they install over earlier alpha and preview builds. They are not Play Store builds. If Android reports a signature mismatch, uninstall the previous build first.

## Build from source

### Prerequisites

- Rust 1.97.1 or newer, with the `aarch64-linux-android` and `wasm32-unknown-unknown` targets
- Bun 1.3.14 or newer
- JDK 21
- Android SDK platform 36, build-tools 36.0.0, and NDK `27.2.12479018`
- `cargo-ndk` 4.1.2
- The Tauri CLI 3.0.0-alpha.5, installed by the root `bun install`

Export `JAVA_HOME` if Android Studio's JDK or `java` on `PATH` is not the JDK 21 installation.

### Debug APK

```sh
rustup target add aarch64-linux-android wasm32-unknown-unknown
cargo install cargo-ndk --version 4.1.2 --locked
bun install

cd crates/koharu
cargo tauri android init
cargo tauri icon icons/icon.json
```

The native runtimes are not committed. Build them with
[`scripts/android-runtimes/README.md`](scripts/android-runtimes/README.md), or extract a
runtime bundle into the generated `jniLibs` directory:

```sh
tar -xzf android-runtimes-arm64-v8a.tar.gz \
  -C gen/android/app/src/main/jniLibs/arm64-v8a
```

Then build the debug APK:

```sh
cargo tauri android build --debug --target aarch64 --apk
```

The APK is written to `gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`.

Release builds and AABs are produced by [`.github/workflows/android.yml`](.github/workflows/android.yml). The CI job fetches the runtime bundle through [`scripts/fetch-android-runtimes.sh`](scripts/fetch-android-runtimes.sh).

## How it works

The Android app reuses Koharu's Rust scene, storage, pipeline, and canvas crates, and the same Next.js UI, inside a Tauri WebView. [`crates/koharu-android`](crates/koharu-android) contains the Android plugin: foreground service, sharing, and WebView integration.

The native runtimes are cross-compiled for `aarch64-linux-android`:

- llama.cpp for local language-model translation
- libtorch with the `koharu-torch` shim for detection, OCR, and inpainting

The app has no Python runtime, desktop sidecar, or GPU server dependency. CPU inference is the default.

## Status and limitations

- Android support is alpha software; expect model-specific and device-specific differences.
- The current release supports `arm64-v8a` only.
- Inpainting is limited to AOT on Android; diffusion-based FLUX and RORem inpainting are not bundled.
- Performance, memory headroom, and thermal behavior depend heavily on the device.
- Remote providers and some archive import formats are still being verified on Android.

The milestone plan, architecture notes, and current open work live in [`Android_build_plan.md`](Android_build_plan.md).

## Contributing

Android changes should target this `android` branch. Desktop changes belong on `main` or upstream.

Before changing code, read [`AGENTS.md`](AGENTS.md). Keep the platform boundary intact, test on a physical device when the change affects native behavior, and run the focused checks:

```sh
bun install
bun run test
bun run lint
```

Device build and runtime details are in [`Android_build_plan.md`](Android_build_plan.md).

## License

Copyright 2025-2026 Mayo Takanashi and Koharu contributors.

Koharu is dual-licensed under the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.

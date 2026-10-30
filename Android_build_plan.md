# Koharu Android - Build Plan

Working document for porting Koharu to Android. Written to be picked up cold: each milestone is self-contained and defines what "done" means. Host-specific paths, the device command recipe, and transient measurements live in `Android_build_plan.local.md`, a local-only companion that is not committed.

- **Default branch:** `android`. Latest preview release `android-alpha-2`. The README documents the Android-only app; the desktop fork is no longer the preview baseline.
- **Historical baseline:** `main` @ `88d07ef0`, workspace version `0.83.5`, Tauri `3.0.0-alpha.5` (git rev `40a2d81d97f5c950e892fa7a6ce8b03064410c4d`), CEF runtime on desktop. This is the fork point, kept for reference only; every milestone since M0 targets `android`.
- **Distribution:** side-load track only. Google Play delivery is out of scope for now; the Play notes below are kept as history for the bundled-runtime decision, not as a current release task.
- **Current state:** the port **processes a full chapter on a physical phone.** Both native runtimes are cross-compiled for `aarch64-linux-android` and bundled (libtorch `2.13` with oneDNN/ACL and the `koharu-torch` shim, llama.cpp `b11642`), so detection -> OCR -> translation -> inpainting -> typeset runs on device. The full 22-page project completes unattended with no `LOW_MEMORY` kill, and the runtime tarball is published on the `android-runtimes` release.
- **Open work:** M3 parity report, the 7.6 GB translation peak against the ~5.5 GB budget (M5), the remaining reference devices, packaging, and M6 landing. See [Next session](#next-session--start-here).

## How to use this document

Milestones: [M0](#m0--it-builds-and-boots), [M1](#m1--platform-services), [M2](#m2--llamacpp-engine), [M3](#m3--libtorch-full-pipeline), [M4](#m4--touch-ui), [M5](#m5--perf-thermal-device-matrix), [M6](#m6--release). Runtime build: [Building the Android runtimes](#building-the-android-runtimes).

Rules that apply to every session:

- Update the checkbox state and add a row to the [Milestone history](#milestone-history).
- Read `AGENTS.md` before changing code. It is binding: typed commands only, no backward compatibility shims, no HTTP RPC layer, one translation prompt path, tests only for real regression boundaries, benchmark on the actual target device.
- Do not hand-edit `gen/android` output. Anything that needs Kotlin lives in the `koharu-android` plugin crate (see [M1](#m1--platform-services)).
- A new platform integration needs an upstream discussion issue before it lands (`CONTRIBUTING.md`). The `android` branch is the development vehicle; landing is [M6](#m6--release).

### Next session - start here

> **Multi-page OOM fixed; 22-page chapter completes.** The defect had three amplifiers: AOT im2col churn, detection at the checkpoint's 1280 px input, and page-major eviction reloading the GGUF per page. The final candidate uses libtorch with oneDNN/ACL for dilated AOT convs, mimalloc for every libtorch tensor allocation, mobile AOT `DEFAULT_MAX_SIDE = 512`, mobile detection input capped at 768, stage-major scheduling on evicting platforms, and the existing one-job-at-a-time eviction policy. Desktop scheduling and residency are unchanged.
>
> **Root causes worth keeping.** Android's `alloc_cpu` hardcodes `memalign`, so `USE_MIMALLOC=ON` alone is a no-op; the patch moves the mimalloc branch ahead of `__ANDROID__`. Keeping tensor churn on Scudo caused the 10 GB `[anon:scudo:secondary]` ratchet. oneDNN is enabled only around AOT through `at::globalContext().setUserEnabledMkldnn`; global dispatch and direct `mkldnn_convolution` both retained too much for detection/AOT. The shim must use `ANDROID_STL=c++_shared`, or libtorch exceptions escape as `Rust cannot catch foreign exceptions`. XNNPACK is ruled out: eager conv dispatches it only for depthwise shapes, and the 1024 px experiment hard-reset the phone; do not retest it.
>
> **Measured on the reference phone.** Standalone AOT at 975x1401: old native kernel at 768 reached 14.2 GB PSS, oneDNN at 768 5.7 GB, oneDNN at 512 2.7 GB. Detection across 3 pages after mimalloc: PSS 214-364 MB, Scudo flat at 4.5 MB. Full 5-page run: peak 7.38 GB, idle 482 MB. Full 22-page run: no `LOW_MEMORY`, peak 7.6 GB during translation, idle 649 MB. The peak is above the ~5.5 GB `RamClass::High` budget, so the remaining optimization is translation-stage peak, not an OOM blocker.
>
> **M2/M3 measurement.** On the reference phone (HONOR Magic V5, `MBH-N49`, 16 GB, Snapdragon 8 Elite) with Q4_K_XL: 21.1 prompt tok/s, 6.57 generated tok/s, 26.7 s/page, 6.1 GB peak RSS for the translator benchmark. The row is recorded in `packages/docs/hardware.mdx` (en/ja/zh); thermal is not measured because sysfs reported an invalid value.
>
> **Runtime tarball published.** The `android-runtimes` release now carries `android-runtimes-arm64-v8a.tar.gz`, so `scripts/fetch-android-runtimes.sh` resolves and the `release` job's `--require` no longer 404s. The README build section fetches that asset directly.
>
> **Still open.** The five-page parity report was not written before the session was finalized; rerun `parity` on device and desktop and compare the reports before publishing M3 parity. The 8 GB mid-range, 12 GB flagship, and 6 GB floor reference devices have no runs yet, and the 7.6 GB full-run peak remains above the ~5.5 GB `RamClass::High` budget.

1. **Fix the multi-page memory defect** (above). Done on device; keep the 22-page run as the regression boundary.
2. **Finish M3 parity.** Re-run the five-page `parity` report on device and desktop with identical pages/models, then compare boxes, masks, OCR text, and translations with `cargo run -p koharu-pipeline --bin parity-diff -- desktop.json android.json`. The Android run needs `TMPDIR` inside app cache and writable model links under `cache/koharu/packages`.
3. **Reduce translation peak** if the 5.5 GB profile budget is a release requirement. The 22-page run survives without it.
4. **Runtime tarball:** published on the `android-runtimes` release. Rebuild with [Building the Android runtimes](#building-the-android-runtimes) and `gh release upload android-runtimes <tarball>` only when the bundled libraries change.
5. **Scale to a chapter.** Done: the full 22-page project processed unattended with no `LOW_MEMORY` kill. Keep the run as the M3 regression boundary and repeat on the 10-page reference set for the M5 matrix.
6. **M2/M3 numbers:** the translator bench is measured (Q4_K_XL) and the reference-phone row is filled across en/ja/zh. The parity report and the other reference devices are still open. KleidiAI does not accelerate Q2_K_XL, so Q4 is the interesting comparison.
7. **Packaging:** the universal debug APK is ~842 MB with the runtimes. Strip the runtime libraries; a per-ABI split does not help an `arm64-v8a`-only build, so the remaining route is an AAB feature module / Play asset delivery (out of scope while the side-load track is current).
8. **Still unverified on device:** BYOK remote providers, a long job with the screen off across the full chapter, and import of a real CBZ/RAR.
9. **M6 landing:** the signed AAB job needs `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`, `SENTRY_AUTH_TOKEN` secrets and `SENTRY_ORG`, then `gh workflow run android.yml --ref <version-tag>` (the `release-pr.yml` dispatch exists). The runtime asset is now published. Then open the upstream issue and split the branch into reviewable PRs.

---

## Locked decisions

1. **First release is fully offline**: detection, OCR, translation, and inpainting all run on-device. BYOK remote providers remain available as an option, not the default.
2. **Torch stays**: cross-compile libtorch 2.13 + the `koharu-torch` shim for `aarch64-linux-android` rather than re-porting models to ONNX.
3. **LLM role matches desktop**: Gemma 4 E4B is the local translation engine (plus the VLM OCR path where relevant), not a chat assistant. The remote Codex agent panel is hidden in the Android v1 UI but its commands stay compiled.
4. **Targets**: phone-first, `minSdk 31` (Android 12+, reliable WebGPU floor), `arm64-v8a` only, 8 GB RAM baseline. **Landing**: local `android` branch now, split into reviewable PRs and an upstream issue later.

---

## Architecture decisions

1. **Runtime split by platform, not a fork.** Desktop keeps `CefRuntime`; Android uses `tauri-runtime-wry` with a `src/lib.rs` + `#[tauri::mobile_entry_point]`. `koharu-app` builds on `tauri::DynRuntime`, so its commands name no concrete runtime; the platform entry installs CEF or Wry with `.runtime(...)`. The mobile entry holds `tauri::async_runtime::handle().inner().enter()` for the process lifetime because setup spawns tasks that probe the Tokio context.
2. **Bundle native runtimes; never download `.so`.** Play's Device & Network Abuse policy forbids downloading executable code, ruling out the "download package -> `dlopen` from cache" flow on Android. A `Bundled` package source resolves bare `libllama.so` from `nativeLibraryDir`; download stays on desktop. Model weights still download (they are data).
3. **Android platform services live in one Tauri mobile plugin crate** (`koharu-android`): Keystore secrets, SAF open/create, share sheet, foreground service, back button. Keeps generated Gradle output untouched and is upstreamable.
4. **Paths are injected, not global.** The app resolves one root for the project library and the runtime package store under `app_data_dir`; config lives at `app_config_dir/config.toml`; logs go through `tauri-plugin-log`'s `LogDir` target. Desktop paths are unchanged.
5. **ML initialization is lazy and non-fatal.** `koharu_ml::init()` moves behind the first pipeline request so the editor always opens; a failed load is reported by the command that needed it and can be retried.
6. **rustls on every platform.** `reqwest` defaults to rustls; the Android blocker was `sentry`'s `transport` feature pulling `native-tls`. Sentry now uses its `reqwest` + `rustls` features, so no `openssl-sys` enters the NDK graph.

---

## Codebase inventory

The "Needs an adapter" rows are the pre-port survey, kept because they explain why each area was touched; M0-M4 closed them.

| Area | Path | Status / why |
|---|---|---|
| Project storage (state slots, blob store, fsync/rename, `fs4` locks, mmap) | `crates/koharu-storage` | Portable as-is |
| Scene / patch / undo (pure Rust document model) | `crates/koharu-scene` | Portable as-is |
| Page preparation: **not** Tauri glue despite the name; platform-neutral, do not port | `crates/koharu-desktop` | Portable as-is |
| Renderer / rasterizer (wgpu/Vello; config path and font discovery need Android sources) | `crates/koharu-renderer`, `crates/koharu-rasterizer` | Portable as-is |
| Import codecs (PNG/JPEG/WebP/ZIP/CBZ/RAR/PDF); export codecs (PNG/PSD/CBZ) | `crates/koharu-app/src/commands/import/mod.rs`, `commands/output.rs` | Portable as-is |
| Canvas wasm (WebGPU in a WebView; capability is conditional, see risks) | `crates/koharu-canvas/src/web.rs`, `packages/bridge/src/canvas.ts` | Portable as-is |
| Logging / URLs | `tauri-plugin-log` (Android full), `tauri-plugin-opener` (URLs only) | Portable as-is |
| Window chrome / layout | `components/app/WindowChrome.tsx`, `TitleBar.tsx`, `KoharuApp.tsx` | Desktop menus, window buttons, drag region, resize handles. |
| Editor layout | `components/editor/Editor.tsx` | Fixed 18/58/24 panes, `minWidth: 900`. |
| Canvas input | `components/editor/CanvasWorkspace.tsx` | Wheel-only zoom, no pinch, no second pointer, no touch slop. |
| Selection handles | `components/editor/SelectionControls.tsx` | 14 px targets, 8 handles + rotate. |
| Page rail | `components/editor/PageRail.tsx` | HTML5 drag reorder, hover-only actions, Shift/Ctrl multi-select, double-click rename. |
| Inspector | `components/editor/Inspector.tsx` | Fixed heights, dense grids, hover-only actions; text editing lives only here. |
| Settings / controls / startup | `components/preferences/*`, `components/controls/*`, `components/app/StartView.tsx`, `StartupView.tsx` | M0 shell is responsive and hides desktop chrome; remaining M4 scope is `dvh` and viewport metadata plus touch polish. |
| Fonts | `crates/koharu-renderer/src/fonts.rs` | HF dataset download must land in app-private storage, not an evictable cache. |
| Background work | `crates/koharu-pipeline`, model/font downloads, exports | Android may freeze or kill the app in background. |

### Blockers

All resolved; kept as a record of why each area was touched.

- CEF runtime (`crates/koharu/src/main.rs`, `koharu-app/src/app.rs`, `tauri.conf.json`), fatal ML init in `app.rs`, `rfd` dialogs (`commands/lifecycle.rs`, `commands/output.rs`, `crates/koharu/src/panic.rs`), desktop paths (`commands/project.rs`, `koharu-config/src/lib.rs`, `koharu-runtime/src/store.rs`), desktop-only plugins in `app.rs` (single-instance, window-state, updater, process), packaging (`identifier: "Koharu"`, `targets: "all"`, no Android config/`gen/android`), TLS (`reqwest` pulls `native-tls` + `openssl-sys`), and no Android secret store (`koharu-secrets/src/lib.rs` uses `keyring`): resolved in M0.
- No Android hardware probe (`koharu-runtime/src/hardware/mod.rs`; `Runtime::discover` bails): resolved in M2, CPU device with the RAM class from `/proc/meminfo` plus a Vulkan device only when `[runtime] android_vulkan` is set.
- No Android native packages (`runtime/packages/{llama,torch,diffusion}.rs`): resolved in M5 by building both locally (see [Building the Android runtimes](#building-the-android-runtimes)). `Diffusion` still has no Android package, so FLUX/RORem inpainting is unavailable there.

### Command surface

51 typed commands in `crates/koharu-app/src/commands/mod.rs` (`bindings()`), bridged to TypeScript in `packages/bridge/src/protocol.ts`: agent (6), lifecycle (12), editing (12), processing (2), output (3), fonts (2), preferences (3), canvas (11). `background_app` and `share` are the additions since the pre-port count.

---

## Mobile model profiles

Translation is **text-only by default**; the mmproj projector (0.99 GB) is loaded only when the vision path is enabled.

| Device RAM | Translation | Detection | OCR | Inpaint | Peak budget |
|---|---|---|---|---|---|
| 6-8 GB | Gemma 4 E2B Q2_K_XL (2.19 GB) | YOLO26s (0.046 GB) | PP-OCRv6 rec (0.08 GB) / MangaOCR (0.46 GB) | AOT (0.02 GB) | ~3.5 GB |
| 8-12 GB | **Gemma 4 E4B Q2_K_XL (3.22 GB)** | YOLO26s (0.046 GB) | MangaOCR (0.46 GB) / PaddleOCR-VL GGUF (1.82 GB) | AOT (0.02 GB) | ~4.5 GB |
| 12-16 GB | **Gemma 4 E4B Q4_K_XL (4.22 GB)** | YOLO26s / RF-DETR Seg 2XL (0.16 GB) | PaddleOCR-VL GGUF / MangaOCR | AOT / LaMa (0.20 GB) | ~5.5 GB |

M3 ships **one platform-agnostic mobile default** (YOLO26s detection, Manga OCR, AOT inpainting) via `DetectionModel`/`OcrModel`/`InpaintingModel::default_for_platform()`, not per-RAM-class selection. The heavier detectors (RF-DETR Seg 2XL, PP-DocLayoutV3) and LaMa stay user-selectable; wiring the RAM class to the defaults is future work (M5).

Reference weights (from `crates/koharu-translator/src/local/catalog.rs` and the `koharu-ml` model repositories): `gemma4-e4b-it` Q4_K_XL 4.22 GB / Q2_K_XL 3.22 GB / mmproj-F16 0.99 GB; `gemma4-e2b-it` Q4_K_XL 2.62 GB / Q2_K_XL 2.19 GB / mmproj-F16 0.99 GB; `qwen3.5-0.8b` Q4_K_XL 0.56 GB + mmproj 0.20 GB (fast profile option). The default desktop profile stays resident at ~9.1 GB and is not viable on mobile. Throughput expectation (not measured here): single-digit tok/s for E4B Q4 on a flagship CPU, so a 30-bubble page is an estimated 1-3 minutes and a 20-page chapter 20-60 minutes; this drives the queue UX and the foreground-service requirement.

---

## Milestones

### M0 - It builds and boots (an installable debug APK with the editor shell, no ML, no dialogs)

- [x] Create the `android` branch; remove `native-tls`/OpenSSL (`sentry` now uses `reqwest` + `rustls`).
- [x] `koharu-app` runtime-agnostic on `tauri::DynRuntime`; mobile entry in `crates/koharu/src/lib.rs` (`#[tauri::mobile_entry_point]`); desktop keeps `main.rs` and the debug CEF devtools port; `#[cfg(desktop)]` around CEF flags and the `rfd` panic dialog.
- [x] Desktop-only `single-instance`/`window-state`/`updater`/`process`; `capabilities/mobile.json`; `Updater.tsx` hidden on Android; `tauri.android.conf.json` (`rs.koharu.app`, `minSdk 31`, generated icons).
- [x] Injectable path roots; lazy ML init; `koharu-secrets` reports no credential instead of failing startup; `koharu-bindgen` passes the NDK sysroot to libclang for the `-sys` build scripts.
- [x] CI workflow `.github/workflows/android.yml`: `cargo ndk check`, debug APK build, artifact upload; `android-m*` tags publish a prerelease APK (`CARGO_PROFILE_DEV_DEBUG=0`, so no native symbolication).

**Exit criteria:** debug APK builds, installs, boots on emulator (Android 15) and on a physical phone; StartView and Settings render with no panic; desktop `cargo check` passes; WebGPU probe on hardware shows an adapter and `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` through the real canvas path, so no native SurfaceView or CPU-composited fallback is needed. The emulator cannot decide this (SwiftShader returns no adapter).

---

### M1 - Platform services (open real files, save keys, survive long jobs)

- [x] `koharu-android` plugin crate (Rust + Kotlin): SAF picker, create-document, share sheet, Keystore, foreground service, back button.
- [x] Picker abstraction in `koharu-app`; `rfd` stays desktop. Import ZIP/CBZ/PDF/RAR/images (unfiltered `*/*`, rejects by extension and content); export PNG/PSD/CBZ; `content://` URIs copy into app cache.
- [x] Keystore-backed store in `koharu-secrets` (undecryptable values dropped and reported unset).
- [x] Foreground service + notification progress for `process` and downloads; partial wakelock; stop on cancel; notification text is data only.
- [x] Back button (close sheet/dialog -> cancel gesture -> background, `packages/koharu/lib/back.ts`); `RunEvent::Resumed`/`Suspended`, orientation, soft-keyboard insets; `INTERNET`; first-run storage note; fonts in app-private storage.

**Exit criteria:** import a CBZ, open the project, export a PNG, save an API key, run a long job with the screen off. Verified on device: CBZ and PDF import, project open, PNG/PSD/CBZ export, and the BYOK credential round trip. A page also finished processing with the screen off while the foreground notification reported progress, and the app returned to the same project on resume. Left: RAR import. **Risk spike resolved:** `tauri-plugin-dialog` returns opaque `content://` URIs, cannot pick a directory, rejects on cancel, and would force a second copy stage, so `koharu-android` owns one SAF surface.

---

### M2 - llama.cpp engine (offline translation with Gemma 4 E4B, measured)

- [x] `aarch64-linux-android` `libllama`/`libmtmd` bundled (NDK r27c, API 31, `GGML_OPENMP=OFF`, `GGML_LLAMAFILE=OFF`, KleidiAI on, 16 KB-aligned) - recipe in [Building the Android runtimes](#building-the-android-runtimes). Pinned to `b11642`, whose headers are byte-identical to `koharu-llama-sys/include`
- [x] `Bundled` package source in `koharu-runtime`; Android arms in `Llama::discover`/`library_names()`; CPU probe always, Vulkan behind `android_vulkan` off by default
- [x] Mobile load profile (`gpu_layers = 0` / Vulkan opt-in offload, mmap, q8 KV cache, performance-core threads, lean `n_ubatch`); RAM class from `/proc/meminfo`; refuse over-budget loads; `koharu-bindgen` Android arm
- [x] Gemma 4 E4B end-to-end translation: detection -> OCR -> translation -> inpainting -> typeset, English page from a Japanese one
- [ ] Benchmark harness + numbers per reference device in `packages/docs/hardware.mdx`. Reference phone row recorded (en/ja/zh), Q4_K_XL: 21.1 prompt tok/s, 6.57 generated tok/s, 26.7 s/page, 6.1 GB translator peak RSS; the full 22-page run peaked at 7.6 GB. Thermal sysfs was invalid (209 °C), so no thermal value. The 8 GB mid-range, 12 GB flagship, and 6 GB floor rows remain open.

**Exit criteria:** a page translated fully on-device in airplane mode, with recorded numbers and no OOM. A page translates correctly and Q4 numbers are recorded on the reference phone; airplane mode and the other reference devices are open. KleidiAI does nothing for Q2_K_XL, so benchmark a Q4 quant.

---

### M3 - libtorch, full pipeline (detection + OCR + inpainting on-device, unattended batch runs)

- [x] Android arms in `Torch::discover`/`library_names()`; map `Vulkan` -> CPU until validated
- [x] Mobile memory policy: evict between stages; extend CPU-lane OOM `recover()` to unload other models and retry once
- [x] Detection model decision: PP-DocLayoutV3 / YOLO26s into one output contract (text/onomatopoeia/bubble/panel + masks); mobile default Comic Layout YOLO26s, RF-DETR Seg 2XL stays desktop. Selectable mobile defaults in `pipeline/src/config.rs` (AOT inpainting default, LaMa optional; MangaOCR on mobile)
- [x] Cross-compile torch 2.13 + `libkoharu-torch.so` (`libtorch_cpu.so` 215 MB, shim 18 MB / 2734 `at_*`/`atg_*` symbols, 16 KB-aligned). Recipe and rationale in [Building the Android runtimes](#building-the-android-runtimes)
- [ ] Parity check: 5-page set, desktop vs Android -> boxes, masks, OCR text, translations; pass when every page produces a report and the diffs are explained or empty (the `koharu-pipeline` `parity` bin landed; compare the two reports with `parity-diff -- desktop.json android.json`. The reference-device reports have not been written, so parity is not published)

**Exit criteria:** a chapter processes unattended with no OOM kills and a parity report is attached. Verified: all four stages load and run on device; the full 22-page project processed unattended with no `LOW_MEMORY` kill, and the Q4 translator benchmark ran. Remaining: the five-page parity report and desktop diff.

---

### M4 - Touch UI (import -> edit -> typeset -> export, one-handed)

- [x] Mobile app bar + overflow menu (`MobileAppBar.tsx`); canvas-first layout with Pages/Inspector/Agent/Activity as bottom sheets (`MobileSheets.tsx`); desktop keeps the 18/58/24 panes
- [x] Gestures in `CanvasWorkspace.tsx`: pinch-zoom, two-finger pan, long-press, touch slop, second-pointer guard (`pointerType === 'touch'`-gated); single-finger draw kept
- [x] Touch targets >=44 px (visual size unchanged); explicit multi-select; pointer-based page reorder; explicit rename; keyboard-aware text editing; `dvh` + `interactive-widget=resizes-content`
- [x] Agent panel hidden on Android v1 (commands stay compiled); Android inpainting picker shows only models with an Android package (`flux2-klein`/`rorem-mixed` hidden)
- [x] Locale keys in all 11 locales (`menu.share`, `menu.more`, `navigator.multiSelect`, `navigator.reorder`); screenshots at 360x800, 412x915, 800x1280 plus a long-label case (title truncates with an ellipsis)

**Exit criteria:** a page can be imported, edited, typeset, and exported on a phone without a keyboard, mouse, or hover. Verified on device: import, mobile shell, canvas render, the Activity sheet's failure surface, the three layouts, and a single page through edit -> typeset -> export (full-resolution PNG through the SAF tree picker). Multi-page processing now completes; touch editing across the full chapter still needs a UI pass.

---

### M5 - Perf, thermal, device matrix

- [ ] Reference devices: 8 GB mid-range, 12 GB flagship, 6 GB floor.
- [ ] Sustained 10-page run: thermal throttling, LMK behavior, battery drain. Criterion: all 10 pages complete with no `LOW_MEMORY` kill and peak PSS within the profile budget (~5.5 GB for `RamClass::High`). Current: the full 22-page run completes with no kill but peaks at 7.6 GB during translation, above budget. The 10-page reference run and battery measurement are still open. Also decide whether RF-DETR survives on mobile and publish the supported matrix.
- [x] Battery-saver profile: `LoadProfile::power_saver` halves decode/prompt threads and batch/ubatch and trims the resident budget 25%; opt in with `[runtime] battery_saver = true`. Off by default; desktop unchanged.

---

### M6 - Release

- [x] CI Android job: JDK, SDK, NDK, `tauri android build`, AAB/APK artifacts, keystore signing, `versionCode` synced from release-please. Preview `build` job publishes the debug APK on `android-m*` tags; the signed AAB job has not run.
- [x] Preview job bundles the native runtimes when published: runs `scripts/fetch-android-runtimes.sh` and extracts into `jniLibs` (preview requires the asset only for `android-m*` tags; `release` always passes `--require`). arm64 ABI split (`--split-per-abi`, not the size fix: the port is `arm64-v8a` only); Sentry Android symbol upload in `.github/sentry.sh`.
- [x] Docs: Android tab in `installation`, `hardware`, `quickstart` across en/ja/zh; reference-phone benchmark row in `hardware` across all three locales.
- [ ] Google Play internal-track upload: out of scope for the current side-load track; notes kept as history. Still manual and unrun (no publisher action is wired, per the no-new-third-party-actions rule).
- [ ] Upstream discussion issue + split PR series (required by `CONTRIBUTING.md`).

---

## Risk register

| Risk | Impact | Mitigation | Where |
|---|---|---|---|
| WebGPU missing in Android WebView, or `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` absent | Canvas unusable | Resolved on hardware (Adreno 8xx): adapter present, `create_canvas` succeeds and renders. No native SurfaceView or CPU fallback needed | M0/closed |
| **A run over more than a page or two is OOM-killed** | A chapter cannot be translated | **Resolved on device.** Three amplifiers: AOT im2col, detection at 1280 px, and page-major eviction reloading the GGUF per page. Fixes: oneDNN/ACL for AOT only, mimalloc for libtorch tensors, mobile AOT 512 px, detection 768 px, stage-major scheduling. The full 22-page run completes with no kill; peak 7.6 GB is still above the 5.5 GB profile target | Closed for OOM; peak budget open |
| libtorch Android build, size, or operator coverage | M3 slips | Resolved: libtorch 2.13 and the shim cross-compile and load on device. Leftover is packaging: the universal debug APK is 842 MB, so it needs stripping; an ABI split does not help an `arm64-v8a`-only build, so the route is an AAB feature module | M5/M6 |
| Gemma 4 E4B memory on 8 GB | OOM | RAM-class profiles, text-only translation, E2B below 8 GB | M2 |
| Sustained decode speed | Poor UX | Queue + foreground service, per-page progress, fast profile (Qwen 3.5 0.8B). KleidiAI does not accelerate Q2_K_XL, so quant choice matters more than the kernel build | M2/M4 |
| SAF URI handling via plugin | Import/export broken | Resolved in M1: `koharu-android` owns one SAF surface and stages copies in app cache | M1/closed |
| Play policy on bundled native runtimes | Distribution | Bundle `.so` only; never download executable code | M0/M6 |
| Detection model contract mismatch | Pipeline quality | Two-way spike with parity comparison | M3 |
| `native-tls`/OpenSSL in the NDK build | Build failure | Switch to rustls first | M0 |
| `rustls-platform-verifier` (via `reqwest`/`hf-hub`) on Android; bundled runtime missing at model load; work that finishes within milliseconds | Downloads hung with no error; pipeline stalled instead of erroring; `ForegroundServiceDidNotStartInTimeException` | Resolved: `hf-hub` (its private `no_redirect_client` bypassed the injected TLS config) is dropped and downloads use the shared client with bundled Mozilla roots; `koharu_ml::features()` makes a stage load fail with the runtime it needs (and `process` catches a residual panic so the job reaches `Failed`); the foreground stop is applied only after the service is foregrounded | M1/M4/closed |

---

## Verification protocol

From `AGENTS.md`, applied per milestone:

- Development uses the smallest relevant check once, debug profile. No full suites, no repeated unchanged runs, no release-profile builds unless explicitly requested.
- Every UI state change needs a screenshot; truncation bugs need a long-label case. Native runtime work needs a structured comparison on identical inputs, not just a green build.
- Model-port work reports device, input size, baseline, result, and correctness delta. End-to-end tests only when explicitly asked for.
- PR text stays short: problem, change, verification.

---

## Building the Android runtimes

Neither runtime is buildable from this repository (`koharu-llama-sys` and `koharu-torch-sys` vendor headers only), and neither upstream publishes an Android asset, so both are produced locally. The authoritative patches, `build`/`configure`/`verify` scripts, pinned revisions, and the README live under `scripts/android-runtimes/`. The build root defaults to `$ANDROID_RUNTIME_BUILD_ROOT` (falling back to `$XDG_CACHE_HOME/koharu-android-runtimes`); nothing under it is committed.

**llama.cpp** - the `koharu-rs/llama` CMake wrapper around upstream `ggml-org/llama.cpp` at tag **`b11642`** (byte-identical to the headers in `crates/koharu-llama-sys/include`, so the pin is not optional). The wrapper only accepts CUDA/HIP/VULKAN/METAL, so `build.sh` applies a patch adding a `CPU` backend (dedupe `ggml-cpu` in the WHOLE_ARCHIVE target list). Configure with the NDK toolchain, `-DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-31`, `GGML_OPENMP=OFF`, `GGML_LLAMAFILE=OFF`, `GGML_CPU_KLEIDIAI=ON`, then build the `llama mtmd` targets. Yields `libllama.so` (55 MB) + `libmtmd.so` (21 MB) with only libm/libdl/libc in `DT_NEEDED`. KleidiAI does nothing for the Q2_K_XL quant: the device logs `no kernel for tensor type tq2_0, not accelerated by KleidiAI`.

**libtorch** - PyTorch `v2.13.0` cross-compiled for Android. `c10/macros/Macros.h` defines `C10_MOBILE` from `__ANDROID__` regardless of CMake, which drops the C++ frontend and autograd the `koharu-torch` shim links against and localizes symbols. The build sets `KOHARU_FULL_LIBTORCH` to opt out of the mobile path. The patch under `scripts/android-runtimes/` carries seven edits:

1. `CMakeLists.txt` gates `INTERN_BUILD_MOBILE` with `AND NOT KOHARU_FULL_LIBTORCH`; `aten/src/ATen/DynamicLibrary.cpp` compiles even under `C10_MOBILE` (the JIT CPU fuser needs it).
2. `cmake/Dependencies.cmake` adds `ANDROID` to the pthreadpool gate (`ATen/ParallelNative.cpp` needs `caffe2::pthreadpool`) and links the full `onnx` library on Android (the non-mobile JIT sources call ONNX's checker and shape inference).
3. `third_party/mimalloc/CMakeLists.txt` drops `-ftls-model=initial-exec` on Android; bionic refuses an initial-exec TLS relocation in a `dlopen`ed library (`TLS symbol "(null)" ... using IE access model`). Check with `readelf -rW libc10.so | grep -c TLS_TPREL64` (must be 0).
4. `caffe2/CMakeLists.txt` compiles `aten/src/ATen/core/dynamic_type.cpp` with `-fvisibility=default`, or `c10::DynamicType::fallback()` stays out of `libtorch_cpu.so`'s dynamic table (`cannot locate symbol _ZNK3c1011DynamicType8fallbackEv`).
5. `aten/src/ATen/native/mkldnn/IDeepRegistration.cpp` allocates oneDNN scratch through `c10::alloc_cpu` on mobile. The mobile CPU allocator returns a data pointer offset by its pre-guard while its context is the base pointer, so `raw_allocate`'s pointer==context assertion fails.
6. `c10/core/impl/alloc_cpu.cpp` moves the `USE_MIMALLOC` branch ahead of `__ANDROID__`. Upstream hardcodes bionic `memalign` on Android, so `-DUSE_MIMALLOC=ON` alone is a no-op and tensor frees ratchet in `[anon:scudo:secondary]`. `free_cpu` already prefers `mi_free`, so alloc/free stay paired.

Other required configuration: host `protoc` via `-DCAFFE2_CUSTOM_PROTOC_EXECUTABLE`, `-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON`, `-DUSE_OPENMP=OFF` (the props list no OpenMP runtime), `-DUSE_KLEIDIAI=OFF`, Eigen under `third_party/eigen`, and `-DBLAS=Eigen -DUSE_MKLDNN=ON -DUSE_MKLDNN_ACL=ON -DDNNL_CPU_RUNTIME=SEQ -DUSE_MIMALLOC=ON -DMI_NO_OPT_ARCH=ON -DUSE_FBGEMM=OFF -DUSE_XNNPACK=OFF -DUSE_PYTORCH_QNNPACK=OFF`, with a static ACL v52.8.0 from `build-acl.sh`. Expect ~1-2 h and ~9 GB for the oneDNN build; the `libtorch_cpu.so` link is the peak. Result after stripping: `libtorch_cpu.so` 184 MB, `libc10.so`, `libtorch.so`, `libtorch_global_deps.so`, all 16 KB-aligned, with no OpenMP, mimalloc, or ACL dynamic dependency. The mobile pipeline enables oneDNN only around AOT inference through `at::globalContext().setUserEnabledMkldnn`; global conv dispatch stays native. Mobile AOT caps input at 512 px and comic-layout detection at 768 px (the checkpoint's 1280 px input made multi-GB detection conv buffers).

**The shim** (`koharu-rs/torch`) builds against the install tree with `find_package(Torch)`. Three Android specifics: pass `-DTorch_DIR=<install>/share/cmake/Torch` (and `CMAKE_FIND_ROOT_PATH_MODE_* = BOTH`) because the NDK toolchain restricts config-mode search to the sysroot; pass `-DANDROID_STL=c++_shared` so libtorch C++ exceptions share the shim's RTTI instead of escaping as a Rust foreign exception; and pass `-DCMAKE_SHARED_LINKER_FLAGS="-Wl,-z,undefs"` because the NDK links shared objects with `-z defs` while desktop allows undefined symbols. Yields `libkoharu-torch.so` (18 MB, 2734 exported `at_*`/`atg_*` symbols).

Finally `scripts/bundle-android-runtimes.sh <llama-dir> <torch-dir>` copies the seven runtime libraries into `jniLibs/arm64-v8a/`; the release tarball also carries the NDK's `libc++_shared.so`. The five torch names and the two llama names must stay identical to the `android` props in `runtime/packages/{torch,llama}.rs`.

---

## Prerequisites

Install: Rust 1.97.1+ (edition 2024) with the `aarch64-linux-android` and `wasm32-unknown-unknown` targets; Bun 1.3.14+; Node for tooling; JDK 21; Android SDK platform 36 + build-tools + cmdline-tools (AGP also pulls `android-37.0` for `compileSdk 37`); NDK r27c; `cargo-ndk` 4.1.2 (`--locked`); the Tauri CLI 3.0.0-alpha.5 (installed by the root `postinstall`). The Tauri CLI only auto-detects Android Studio's JBR or `java` on `PATH`; export `JAVA_HOME` on hosts with neither.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-linux-android wasm32-unknown-unknown
cargo install cargo-ndk --version 4.1.2 --locked
curl -fsSL https://bun.sh/install | bash
sdkmanager "platforms;android-36" "build-tools;36.0.0" "ndk;27.2.12479018"
bun install   # also installs the Tauri CLI
```

Device testing: `adb reverse tcp:3000 tcp:3000` is only needed for `tauri android dev` with a dev server. The built debug APK embeds the frontend and serves `http://tauri.localhost/`; it needs neither `adb reverse` nor a running dev server.

---

## Open questions

- Detection: wiring landed for all three detectors; the mobile default is YOLO26s. The final quality-vs-latency pick and whether to ship Vulkan at all (or CPU-only for v1) both need the reference-device numbers (M2/M5).
- **What held ~10 GB of native memory during a multi-page run?** Tensor churn retained by bionic Scudo, fed by 1280 px detection and AOT im2col, plus page-major eviction reloading the GGUF per page. Mimalloc-backed libtorch tensors, 768 px detection, 512 px AOT, and stage-major scheduling remove the OOM; the remaining 7.6 GB translation peak is a budget issue, not a kill.
- APK size budget: 842 MB universal debug APK with the runtimes. Strip the runtime libraries; a per-ABI split does not help an `arm64-v8a`-only build, so the remaining route is an AAB feature module / Play asset delivery. The runtime tarball route is already wired into the preview job.
- OCR language selection: the mobile default is Manga OCR (Japanese). Auto-selecting MangaOCR vs. PaddleOCR-VL GGUF per page language is not implemented.
- Android TLS trust: `configure_reqwest_tls` uses bundled Mozilla roots, so user-installed and corporate CAs are ignored. Revisit only if a user needs them (would mean initializing `rustls-platform-verifier` over JNI).

---

## Milestone history

One row per milestone. Host-specific detail and session notes live in the local file.

| Milestone | When | Result |
|---|---|---|
| Plan | 2026-10-21 | Repository surveyed; blockers, model profile, and milestones defined. |
| M0 | 2026-10-21 | Runtime-agnostic `koharu-app`, platform service split, injected paths, lazy ML init, secrets fail soft, NDK sysroot for bindgen, CI workflow. APK installs and boots; WebGPU confirmed on hardware. |
| M1 | 2026-10-21 | `koharu-android` plugin (SAF, Keystore, foreground service, back button), picker abstraction, app-private fonts, locale strings. Import/export/key round trip verified on device. |
| M2 | 2026-10-21 | Android hardware probe, bundled llama packages, mobile load profile, RAM classes, `bench` bin, docs. Local models translate a page end-to-end on device. |
| M3 | 2026-10-22 | Bundled torch packages, one detection output contract, platform-aware model defaults, memory policy on the accelerator gate, `parity` bin. Full pipeline runs on device. |
| M4 | 2026-10-22 | Mobile shell (app bar + bottom sheets), touch gestures, >=44 px targets, `dvh` viewport, Android inpainting picker, share command, 11 locales, screenshots. One page edits/typesets/exports on device. |
| M5 | 2026-10-22 | Native runtimes built and bundled; power-saver profile; text-only translation default. |
| M6 | 2026-10-22 | Release CI job, runtime fetch into `jniLibs`, arm64 ABI split, Sentry symbol upload, docs. Signed AAB job unrun. |
| Multi-page memory | 2026-10-23 to 2026-10-24 | OOM removed. Root causes: mimalloc bypassed on Android so tensor frees ratcheted in Scudo; 1280 px detection and AOT im2col fed the churn; page-major eviction reloaded the GGUF per page. Fixes: mimalloc for libtorch, AOT-only oneDNN/ACL, 768 px mobile detection, 512 px mobile AOT, stage-major scheduling. 22-page unattended run passes with no LOW_MEMORY; peak 7.6 GB, idle 649 MB. Q4 bench recorded; parity report still open. |
| Docs and runtime bundle | 2026-10-27 | `android-runtimes` tarball published; README build section fetches it. Reference-phone benchmark row filled in `hardware.mdx` (en/ja/zh) with the translator numbers, the 7.6 GB full-run peak noted above the ~5.5 GB target, and thermal marked not measured. `parity-diff` documented as the report comparison step. M3 parity, the 8 GB / 12 GB / 6 GB reference rows, and the M5 peak budget remain open. |

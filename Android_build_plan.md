# Koharu Android — Build Plan

Working document for porting Koharu to Android. Written to be picked up cold in a new
session: each milestone below is self-contained, lists the files it touches, and defines
what "done" means.

- **Repository:** `$HOME/Koharu/koharu`
- **Baseline:** `main` @ `88d07ef0`, workspace version `0.83.5`, Tauri `3.0.0-alpha.5`
  (git rev `40a2d81d97f5c950e892fa7a6ce8b03064410c4d`), CEF runtime on desktop.
- **Status date:** 2026-10-22
- **Current state:** the `android` branch carries M0–M6 code; the port is code-complete and
  the preview APK is published (`android-m4-preview-2`). M0–M3 landed (platform services, the
  bundled llama/torch consumer side, the mobile load profile and RAM budgets, the
  YOLO26s/Manga-OCR/AOT mobile defaults, the `parity`/`bench` harnesses, docs). M4 landed: the
  mobile editor (app bar + Pages/Inspector/Activity bottom sheets, touch gestures, 44 px
  targets, the `share` command, 11 locales). M5 landed (`[runtime] battery_saver`). M6 landed
  as configuration: the preview `build` job publishes the debug APK on `android-m*` tags; the
  signed AAB job is unrun and needs the signing secrets.
  **Device status:** the preview installs, boots, and **imports a page through the system
  picker** on a physical phone (Android 16, arm64), and **downloads Hugging Face files**
  (`config.json` + `model.safetensors` land in the store). The one blocking gap is the
  external `koharu-rs/{llama,torch}` Android assets: they are not published and not bundled,
  so no stage can load. That failure mode is now correct: the run **fails the job with a
  readable error** naming the missing runtime instead of stalling (fixed 2026-10-22; the
  stage load is guarded before the lazy `-sys` loader can panic, and the job task catches any
  residual panic). See [Next session](#next-session--start-here) for the exact handoff. The
  port targets Android and desktop; a non-Android mobile build still compiles with the pre-M1
  "picker unavailable" behavior but is not supported.

## How to use this document

| Session | Milestone | Start here |
|---|---|---|
| 1 | Toolchain + M0 | [Prerequisites](#prerequisites), [M0](#m0--it-builds-and-boots) |
| 2 | M1 | [M1](#m1--platform-services) |
| 3 | M2 | [M2](#m2--llamacpp-engine) |
| 4 | M3 | [M3](#m3--libtorch-full-pipeline) |
| 5 | M4 | [M4](#m4--touch-ui) |
| 6 | M5–M6 | [M5](#m5--perf-thermal-device-matrix), [M6](#m6--release) |
| 7 | Native runtimes + device pass | [Next session](#next-session--start-here), [Debugging on a device](#debugging-on-a-device) |
| 8 (next) | Native runtimes + device pass | [Next session](#next-session--start-here) |

Rules that apply to every session:

- Update the checkbox state and append to the [Session log](#session-log) when you finish.
- Read `AGENTS.md` before changing code. It is binding: typed commands only, no backward
  compatibility shims, no HTTP RPC layer, one translation prompt path, tests only for real
  regression boundaries, benchmark on the actual target device.
- Do not hand-edit `gen/android` output. Anything that needs Kotlin lives in the
  `koharu-android` plugin crate (see [M1](#m1--platform-services)).
- A new platform integration needs an upstream discussion issue before it lands
  (`CONTRIBUTING.md`). The `android` branch is the development vehicle; landing is
  [M6](#m6--release).

### Next session — start here

The port is code-complete through M6; what remains is the external native runtime and the
device pass. In order:

1. **Produce the native runtimes — still the only thing blocking a working pipeline.** Build
   `libc10.so` + `libtorch_global_deps.so` + `libtorch_cpu.so` + `libtorch.so` +
   `libkoharu-torch.so` (`koharu-rs/torch`) and `libllama.so` + `libmtmd.so`
   (`koharu-rs/llama`) for `aarch64-linux-android`, then run
   `scripts/bundle-android-runtimes.sh <koharu-rs-checkout>` to copy them into
   `crates/koharu/gen/android/app/src/main/jniLibs/arm64-v8a/` (git-ignored; `tauri android
   build` copies whatever is there). Re-checked 2026-10-22: neither repository publishes an
   Android asset, and neither can be built from this repository — `koharu-llama-sys` vendors
   headers only and `koharu-torch-sys/libtch` is two headers. The torch build must pass
   `-DUSE_OPENMP=OFF` to match the `android` props; see
   [M3](#m3--libtorch-full-pipeline) for the verified `DT_NEEDED` set.
2. **Run the pipeline on the phone.** With the libraries present, run detection → OCR →
   translation → inpainting on a real page and confirm each stage loads and completes. Without
   them the run now fails fast with `<model> needs the Torch runtime, which is not available on
   this device` — verified on the phone 2026-10-22.
3. **M4 exit gate:** run import → edit → typeset → export one-handed and capture screenshots
   at 360×800, 412×915, and 800×1280 (plus a long-label case).
4. **M1 exit criteria still open:** export PNG/PSD/CBZ through the tree picker, save a BYOK
   key, and run a long job with the screen off. The foreground-service crash that a
   fast-failing job used to trigger is fixed, so a long job can now be observed end to end.
5. **M0 leftover:** record `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` from the phone's WebView.
6. **M2/M3 numbers:** run `bench` and `parity` on the reference devices and fill
   `packages/docs/hardware.mdx` (en/ja/zh).
7. **M6 landing:** the signed AAB job needs `ANDROID_KEYSTORE_BASE64`,
   `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`, `SENTRY_AUTH_TOKEN`
   secrets and the `SENTRY_ORG` variable, then `gh workflow run android.yml --ref <version-tag>`
   (the `release-pr.yml` dispatch already exists); then open the upstream issue and split the
   branch into reviewable PRs.

---

## Locked decisions

1. **First release is fully offline**: detection, OCR, translation, and inpainting all run
   on-device. BYOK remote providers remain available as an option, not the default.
2. **Torch stays**: cross-compile libtorch 2.13 + the `koharu-torch` shim for
   `aarch64-linux-android` rather than re-porting models to ONNX.
3. **LLM role matches desktop**: Gemma 4 E4B is the local translation engine (plus the
   VLM OCR path where relevant), not a chat assistant. The remote Codex agent panel is
   hidden in the Android v1 UI but its commands stay compiled.
4. **Targets**: phone-first, `minSdk 31` (Android 12+, reliable WebGPU floor),
   `arm64-v8a` only, 8 GB RAM baseline.
5. **Landing**: local `android` branch now, split into reviewable PRs and an upstream
   issue later.

---

## Codebase inventory

### Portable as-is

| Area | Path | Notes |
|---|---|---|
| Project storage | `crates/koharu-storage` | State slots, blob store, fsync/rename, `fs4` locks, mmap. Assumes an app-private directory. |
| Scene / patch / undo | `crates/koharu-scene` | Pure Rust document model. |
| Page preparation | `crates/koharu-desktop` | **Not** Tauri glue despite the name; platform-neutral. Do not port or replace. |
| Renderer / rasterizer | `crates/koharu-renderer`, `crates/koharu-rasterizer` | wgpu/Vello; config path and font discovery need Android sources. |
| Import codecs | `crates/koharu-app/src/commands/import/mod.rs` | PNG/JPEG/WebP/ZIP/CBZ/RAR/PDF via pure-Rust crates. |
| Export codecs | `crates/koharu-app/src/commands/output.rs` | PNG/PSD/CBZ. |
| Canvas wasm | `crates/koharu-canvas/src/web.rs`, `packages/bridge/src/canvas.ts` | WebGPU in a WebView; capability is conditional (see risks). |
| Logging / URLs | `tauri-plugin-log` (Android full), `tauri-plugin-opener` (URLs only) | Already fine. `login_agent` opens the URL through `handle.opener().open_url`, so Android hands it to the system handler. |

### Needs an adapter

| Area | Path | Why |
|---|---|---|
| Window chrome / layout | `packages/koharu/components/app/WindowChrome.tsx`, `TitleBar.tsx`, `KoharuApp.tsx` | Desktop menus, window buttons, drag region, resize handles. |
| Editor layout | `components/editor/Editor.tsx` | Fixed 18/58/24 panes, `minWidth: 900` from `tauri.conf.json`. |
| Canvas input | `components/editor/CanvasWorkspace.tsx` (959 lines) | Wheel-only zoom, no pinch, no second pointer, no touch slop, `onWheel`-only brush sizing. |
| Selection handles | `components/editor/SelectionControls.tsx` | 14 px targets, 8 handles + rotate. |
| Page rail | `components/editor/PageRail.tsx` (581 lines) | HTML5 drag reorder, hover-only actions, Shift/Ctrl multi-select, double-click rename. |
| Inspector | `components/editor/Inspector.tsx` (1073 lines) | Fixed heights, dense grids, hover-only actions; text editing lives only here. |
| Settings | `components/preferences/SettingsPage.tsx`, `PreferenceFields.tsx` | M0 shell is responsive: the sidebar collapses to a stacked nav with a three-column tab strip, `PreferenceRow` is one column below `md`, and safe-area insets apply. The remaining gap is touch polish (target sizes, keyboard handling), which is M4, not the layout. Providers is available on mobile since M1: credentials persist in the Keystore-backed store. |
| Controls | `components/controls/*` | `FontPicker.tsx` 543 lines; popovers sized for pointer use. |
| Startup / views | `components/app/StartView.tsx`, `StartupView.tsx`, `AboutDialog.tsx` | M0 hides the desktop window chrome on mobile and applies safe-area insets. `StartupView` still uses `h-screen w-screen`; the remaining M4 scope is `dvh` and viewport metadata, not the chrome. |
| Fonts | `crates/koharu-renderer/src/fonts.rs` | HF dataset download must land in app-private storage, not an evictable cache. |
| Background work | `crates/koharu-pipeline`, model/font downloads, exports | Android may freeze or kill the app in background. |

### Blockers

| Blocker | Evidence | Status |
|---|---|---|
| CEF runtime | `crates/koharu/src/main.rs` (`#[tauri_runtime_cef::cef_entry_point]`), `crates/koharu-app/src/app.rs` (`Builder::<CefRuntime>`, `Context<CefRuntime>`), 6 command modules, `tauri.conf.json` desktop window. | Resolved in M0 |
| ML init is fatal | `app.rs` `initialize()` calls `koharu_ml::init()` and `Pipeline::load` with `.expect(...)`; frontend waits on `Initialization` and stays on the startup spinner. | Resolved in M0 |
| No Android hardware probe | `crates/koharu-runtime/src/hardware/mod.rs:26-43` probes only macOS/Linux/Windows; `Runtime::discover` then `bail!("no device supports the requested runtime features")` (`runtime/mod.rs:70`). | Open |
| No Android native packages | `runtime/packages/{llama,torch,diffusion}.rs` publish only Windows/Linux/macOS assets. No CPU llama package exists at all. | Open |
| No Android secret store | `crates/koharu-secrets/src/lib.rs:59-105` uses `keyring` (apple/linux-keyutils/windows), loaded during `Preferences::load()` on `subscribe`. | Resolved in M0 |
| `rfd` dialogs | `crates/koharu-app/src/commands/lifecycle.rs:351`, `commands/output.rs:58`, `crates/koharu/src/panic.rs:25`. | Resolved in M0 |
| Desktop paths | `commands/project.rs:185` (`dirs::document_dir()/Koharu`), `koharu-config/src/lib.rs:375` (`~/.koharu/config.toml`), `koharu-runtime/src/store.rs:52` (`dirs::cache_dir()`). | Resolved in M0 |
| Desktop-only plugins | `app.rs:83-102`: single-instance, window-state, updater, process. | Resolved in M0 |
| Packaging | `identifier: "Koharu"`, `targets: "all"`, no Android icons/config/`gen/android`. | Resolved in M0 |
| TLS | `reqwest` pulls `native-tls` + `openssl-sys`; no OpenSSL story on Android. | Resolved in M0 |

### Command surface

49 typed commands in `crates/koharu-app/src/commands/mod.rs` (`bindings()`), bridged to
TypeScript in `packages/bridge/src/protocol.ts`: agent (6), lifecycle (11), editing (12),
processing (2), output (2), fonts (2), preferences (3), canvas (11).

---

## Architecture decisions

1. **Runtime split by platform, not a fork.** Desktop keeps `CefRuntime`; Android uses
   `tauri-runtime-wry` with a `src/lib.rs` + `#[tauri::mobile_entry_point]`.
   `koharu-app` builds on `tauri::DynRuntime`, so its commands and setup name no concrete
   runtime; the platform entry installs CEF or Wry with `.runtime(...)`. The mobile entry
   holds `tauri::async_runtime::handle().inner().enter()` for the process lifetime because
   setup spawns tasks that probe the Tokio context.
2. **Bundle native runtimes; never download `.so`.** Play's Device & Network Abuse policy
   forbids downloading executable code, which rules out the current
   "download package → `dlopen` from cache" flow on Android. Add a `Bundled` package
   source that resolves bare `libllama.so` from `nativeLibraryDir`; keep download for
   desktop. Model weights still download normally (they are data).
3. **Android platform services live in one Tauri mobile plugin crate** (`koharu-android`):
   Keystore secrets, SAF open/create, share sheet, foreground service, back button. Keeps
   generated Gradle output untouched and is upstreamable.
4. **Paths are injected, not global.** The app resolves one root for the project library and
   the runtime package store under `app_data_dir`; configuration lives at
   `app_config_dir/config.toml`; logs go through `tauri-plugin-log`'s `LogDir` target.
   Desktop paths are unchanged.
5. **ML initialization becomes lazy and non-fatal.** Move `koharu_ml::init()` behind the
   first pipeline request so the editor always opens; a failed load is reported by the
   command that needed it and can be retried.
6. **Use rustls on every platform.** `reqwest` already defaults to rustls; the Android
   blocker was `sentry`'s `transport` feature, which pulls `native-tls`. Sentry now uses
   its `reqwest` + `rustls` features, so no `openssl-sys` enters the NDK graph.

---

## Mobile model profiles

Translation is **text-only by default**; the mmproj projector (0.99 GB) is loaded only when
the vision path is enabled.

| Device RAM | Translation | Detection | OCR | Inpaint | Peak budget |
|---|---|---|---|---|---|
| 6–8 GB | Gemma 4 E2B Q2_K_XL (2.19 GB) | YOLO26s (0.046 GB) | PP-OCRv6 rec (0.08 GB) / MangaOCR (0.46 GB) | AOT (0.02 GB) | ~3.5 GB |
| 8–12 GB | **Gemma 4 E4B Q2_K_XL (3.22 GB)** | YOLO26s (0.046 GB) | MangaOCR (0.46 GB) / PaddleOCR-VL GGUF (1.82 GB) | AOT (0.02 GB) | ~4.5 GB |
| 12–16 GB | **Gemma 4 E4B Q4_K_XL (4.22 GB)** | YOLO26s / RF-DETR Seg 2XL (0.16 GB) | PaddleOCR-VL GGUF / MangaOCR | AOT / LaMa (0.20 GB) | ~5.5 GB |

M3 ships **one platform-agnostic mobile default** (YOLO26s detection, Manga OCR, AOT
inpainting) via `DetectionModel`/`OcrModel`/`InpaintingModel::default_for_platform()`, not
per-RAM-class selection. The heavier detectors (RF-DETR Seg 2XL, PP-DocLayoutV3) and LaMa
stay user-selectable; wiring the RAM class to the defaults is future work (M5).

Reference weights (from `crates/koharu-translator/src/local/catalog.rs` and the
`koharu-ml` model repositories):

- `gemma4-e4b-it`: Q4_K_XL 4.22 GB, Q2_K_XL 3.22 GB, mmproj-F16 0.99 GB,
  `unsloth/gemma-4-E4B-it-qat-GGUF`
- `gemma4-e2b-it`: Q4_K_XL 2.62 GB, Q2_K_XL 2.19 GB, mmproj-F16 0.99 GB
- `qwen3.5-0.8b`: Q4_K_XL 0.56 GB + mmproj 0.20 GB (fast profile option)
- Default desktop profile stays resident at ~9.1 GB and is not viable on mobile.

Throughput expectation: single-digit tok/s for E4B Q4 on a flagship CPU. A 30-bubble page
is roughly 1–3 minutes; a 20-page chapter 20–60 minutes. This drives the queue UX and the
foreground-service requirement.

---

## Milestones

### M0 — It builds and boots

**Goal:** an installable debug APK with the editor shell, no ML, no dialogs.

- [x] Create the `android` branch from `main`
- [x] Remove `native-tls`/OpenSSL from the Android graph: `reqwest` already defaults to
      rustls; the source was `sentry`'s `transport` feature, which now uses `reqwest` + `rustls`
- [x] Make `koharu-app` runtime-agnostic on `tauri::DynRuntime`; add `tauri-runtime-wry`
- [x] `crates/koharu/src/lib.rs` mobile entry, `#[tauri::mobile_entry_point]`; desktop
      `main.rs` keeps its behavior and the debug CEF devtools port `:4000`
- [x] `#[cfg(desktop)]` around: CEF Linux flags, `rfd` panic dialog
- [x] Keep `single-instance`, `window-state`, `updater`, `process` on desktop targets only; add
      `crates/koharu/capabilities/mobile.json`; hide `Updater.tsx` on Android
- [x] `tauri.android.conf.json`: identifier `rs.koharu.app` (overlay only — desktop bundle id
      unchanged), `minSdk 31`; launcher icons are the generated defaults
- [x] Injectable path roots for `ProjectLibrary` (`commands/project.rs`),
      `koharu-config` (`path()`), `koharu-runtime::Store::configure`
- [x] Lazy ML init behind the first pipeline request; startup no longer waits for models
- [x] `koharu-bindgen`: pass the NDK sysroot to libclang so the `-sys` build scripts can
      generate Android bindings
- [x] `koharu-secrets`: Android reports no credential instead of failing `Preferences` startup
- [x] CI workflow added (`.github/workflows/android.yml`): `cargo ndk check`, debug APK
      build, artifact upload; pushing a tag matching `android-m*` builds the debug arm64
      APK and publishes it as a prerelease with the APK asset

  The preview job sets `CARGO_PROFILE_DEV_DEBUG=0`, so the preview APK is much smaller than
  the 1.6 GB local dev APK; native stack symbolication is unavailable in that preview.
  The preview is `arm64-v8a` only and signed with the CI debug key, so installing a second
  preview may require uninstalling the first.
- [ ] First CI run to green

**Exit criteria:**

- [x] Debug APK builds, installs, and boots on the emulator (Android 15, `sdk_gphone64_x86_64`).
      Uninstall first: installing over the previous build fails with
      `INSTALL_FAILED_INSUFFICIENT_STORAGE` (debug APK is 1.6 GB), so
      `adb uninstall rs.koharu.app` then `adb install ...`
- [x] StartView and Settings render with no panic and no startup hang. Settings uses the
      stacked mobile layout with safe-area padding; the storage hint and desktop title bar
      are gone on mobile. Assets are served from `http://tauri.localhost/`, no dev server.
- [x] Desktop `cargo check` passes after the fixes
- [x] Install and boot on a physical phone (Android 16, arm64; CI preview APK)
- [x] WebGPU probe on hardware (2026-10-22, test phone, Android 16, Chrome/155
      WebView). `navigator.gpu` present; `requestAdapter({powerPreference:'high-performance'})`
      returns an adapter reported as `vendor=qualcomm, architecture=adreno-8xx`,
      `maxTextureDimension2D=16384`, with `core-features-and-limits`, `subgroups`,
      `float32-filterable`, `texture-compression-astc/bc/etc2`, `timestamp-query`.
      `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` is a wgpu *downlevel* flag and has no JS feature
      name, so it was confirmed through the real code path instead: the canvas is created and
      renders the imported page, and `koharu-canvas::create_canvas` returns an error for an
      adapter without that flag — so the flag is present on this GPU. The canvas is
      `1060 × 1881` backing at DPR 3 (`353 × 792` CSS). Device page size is 4 KB
      (`getconf PAGE_SIZE`), so 16 KB alignment is not enforced by this phone.

**Spike to run in this milestone:** log `navigator.gpu` availability and adapter
`UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` from the Android WebView. This single result decides
whether `koharu-canvas` works as-is or needs a fallback (see risks).

Emulator result: `navigator.gpu` is present but `navigator.gpu.requestAdapter()` returns
`null` — the emulator renders in software (SwiftShader), so
`UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` could not be checked. The emulator cannot decide the
canvas path; the physical-device probe above resolves the spike. **The canvas path is
supported on Adreno; no native SurfaceView or CPU-composited fallback is needed.**

---

### M1 — Platform services

**Goal:** the app can open real files, save keys, and survive long jobs.

- [x] New `koharu-android` plugin crate (Rust + Android/Kotlin): SAF picker, create-document,
      share sheet, Keystore, foreground service, back button
      (the share sheet has no call site yet; it is consumed by the M4 share action)
- [x] Picker abstraction in `koharu-app`; `rfd` stays the desktop implementation.
      Import: multi-select ZIP/CBZ/PDF/RAR/images. Export: PNG/PSD/CBZ.
      Handle `content://` URIs by copying into app cache before decode. The Android import
      picker is unfiltered (`*/*`) because providers report `.cbz`/`.rar` as generic MIME
      types; the importer rejects unsupported inputs by extension and content.
- [x] Keystore-backed store in `koharu-secrets` (same get/set/delete API; BYOK keys persist).
      A value that cannot be decrypted (lost Keystore key, Android Auto Backup restore) is
      dropped and reported as unset instead of failing `Preferences::load`
- [x] Foreground service + notification progress for `process` and model/font downloads;
      partial wakelock; stop on cancel. The notification title falls back to the application
      label and its text is data only (download or model name) so no English copy is
      hardcoded in Rust
- [x] Back button: close sheet/dialog → cancel gesture → background the app
      (`packages/koharu/lib/back.ts` gives each dismissible surface a handler; Settings
      saves before closing, the delete confirmation closes, and the app backgrounds when no
      surface consumes the press. M4 adds sheets and gesture cancellation)
- [x] `RunEvent::Resumed`/`Suspended`, orientation changes, soft-keyboard insets
      (Wry reports these as `WindowEvent::Resumed`/`Suspended`. Orientation is already in the
      activity's `configChanges` and safe-area insets are applied in M0. Keyboard-aware insets
      stay with the M4 viewport work.)
- [x] `INTERNET` permission; first-run storage note
- [x] Fonts: fetch into app-private storage, not an evictable cache path
      (the runtime store is configured under `app_data_dir` on mobile)

**Exit criteria:** import a CBZ, open the project, export a PNG, save an API key, run a long
job with the screen off. Verified on the phone: import (a JPG through the system picker),
open, PNG export through the tree picker, CBZ export through create-document (a valid ZIP
containing `0001_<page label>.png`), and the BYOK credential round trip. Left: a CBZ/RAR
import, PSD export, and the long job with the screen off — the last one needs the native
runtimes, because every stage currently fails its load.

Device progress: import is verified — a real manga JPG imported through the system picker into
a new project. Export is verified — a PNG came out through the system tree picker at full
resolution. The Keystore-backed credential store is verified — a provider credential persisted
across a WebView reload (shown as `Configured`, never echoed back) and cleared again from the
settings page. The foreground-service crash that a fast-failing job triggered is fixed (see the
risk register). Remaining: PSD/CBZ export and the long job with the screen off, which needs a
stage that can actually load (the native runtimes). The import picker on the phone is the M1
implementation, not the M0 stub; the earlier "importing files is not supported" report came
from the M0 preview.

**Risk spike:** resolved. `tauri-plugin-dialog` on Android returns opaque `content://` URIs
and cannot pick a directory, which the PNG/PSD export needs. The plugin also rejects on
cancel and would force a second copy stage. `koharu-android` therefore owns one SAF surface:
multi-select document import, document-tree export, and create-document export.

---

### M2 — llama.cpp engine

**Goal:** offline translation with Gemma 4 E4B, measured.

- [ ] Publish `aarch64-linux-android` `libllama`/`libmtmd` from `koharu-rs/llama`
      (NDK r27c, API 31, `GGML_OPENMP=OFF`, `GGML_LLAMAFILE=OFF`, KleidiAI on,
      `-Wl,-z,max-page-size=16384` for Play's 16 KB page rule)
      — external to this repository; the consumer side assumes both libraries are in the
      APK's `jniLibs`. Re-checked 2026-10-22: `koharu-rs/llama` publishes only
      windows/linux/macos assets (latest `b11786`), so the Android asset does not exist yet.
      This repository cannot produce it — `crates/koharu-llama-sys` vendors headers only
      (`include/*.h`), with no llama.cpp sources.
- [x] `Bundled` package source in `koharu-runtime`; Android arms in `Llama::discover` and
      `library_names()`
- [x] Android hardware probe: CPU device always; Vulkan behind a setting, off by default
      (`android_vulkan` in the runtime config section; Adreno Vulkan/OpenCL is frequently
      slower than CPU for small models and unstable)
- [x] Mobile load profile: `gpu_layers = 0` on the CPU path / Vulkan opt-in offload, mmap,
      q8 KV cache, performance-core thread count, lean `n_ubatch`
- [x] RAM class detection (`/proc/meminfo`) → profile selection; refuse loads that exceed
      budget with a readable error. (`ActivityManager.getMemoryClass()` is not consulted;
      `/proc/meminfo` is self-contained and reports the conservative class)
- [x] `koharu-bindgen`: Android arm in `already_loaded_library` (`rewrite.rs:146-151`),
      and the `-sys` crates declare `libc` for `android`
- [ ] Gemma 4 E4B end-to-end translation; BYOK providers verified on device
- [ ] Benchmark harness + numbers per reference device (tok/s, page latency, peak RSS,
      thermal status); document in `packages/docs/hardware.mdx`
      — harness landed (the `koharu-translator` `bench` bin) and the docs carry a results
      table template; the numbers need the reference devices

**Exit criteria:** a page translated fully on-device in airplane mode, with recorded numbers
and no OOM.

---

### M3 — libtorch, full pipeline

**Goal:** detection + OCR + inpainting on-device, unattended batch runs.

- [x] Android arms in `Torch::discover` / `library_names()`; map `Vulkan` → CPU until validated
      (`Torch::AndroidCpu` is a bundled package: no download, bare sonames, empty dependencies)
- [x] Mobile memory policy: one resident stage, evict between stages; extend CPU-lane OOM
      `recover()` (`pipeline/src/accelerator.rs`) to unload other models and retry once
      (`AcceleratorGate` now serializes on mobile via `MemoryPolicy::for_platform()` and evicts
      other stages before every load; `recover()` no longer short-circuits on the CPU lane)
- [x] Detection model decision. Wire PP-DocLayoutV3 (0.13 GB) / YOLO26s (0.046 GB) into the
      same output contract (text/onomatopoeia/bubble/panel + masks). The detection stage now
      normalizes every detector onto a pipeline-owned record; YOLO26s maps `balloon`/`frame`/
      `text` → bubble/panel/text and PP-DocLayoutV3 rasterizes its polygon masks, keeping only
      text regions. Mobile default is Comic Layout YOLO26s (comic-native classes, smallest
      footprint); RF-DETR Seg 2XL stays the desktop default. **Quality vs latency pick still
      needs the reference devices** (M2/M5 benchmark item).
- [x] Selectable mobile defaults in `pipeline/src/config.rs`; AOT default inpainting, LaMa optional
      (`DetectionModel`/`OcrModel`/`InpaintingModel::default_for_platform()`)
- [x] OCR defaults: MangaOCR on mobile, PaddleOCR-VL on desktop (reuses the llama runtime, no
      torch). Per-page language selection is not implemented; the mobile default assumes the
      Japanese manga target.
- [ ] Cross-compile torch 2.13 (`libtorch_cpu.so`) + `libkoharu-torch.so` for
      `aarch64-linux-android`; publish to `koharu-rs/torch`; bundle via jniLibs or an AAB
      feature module
      — external to this repository; the consumer side is done and `cargo ndk check -p koharu`
      is green. The APK needs both libraries in `jniLibs` before a device run. The `android`
      props in `runtime/packages/torch.rs` were verified on 2026-10-22 against `readelf -d` of
      the published `v2.13.0.7` Linux CPU asset: the set is correct **provided the Android
      build passes `-DUSE_OPENMP=OFF`**. Otherwise `libtorch_cpu.so` and
      `libtorch_global_deps.so` need an OpenMP runtime, which on the NDK is `libomp.so` (not a
      platform library), so it would have to be bundled into `jniLibs` and added to the props.
      `libtorch_global_deps.so` is a dlopen root only — nothing `DT_NEEDED`s it, but upstream
      builds it unconditionally, so it stays in the props. `crates/koharu-torch-sys` vendors
      the shim headers only, so `libkoharu-torch.so` cannot be produced here either.
      Re-checked 2026-10-22: `koharu-rs/torch` publishes no Android asset.
- [ ] Parity check: 5-page set, desktop vs Android → boxes, masks, OCR text, translations;
      record diffs
      — the `koharu-pipeline` `parity` bin landed (JSON report); the two reports need the
      reference device

**Exit criteria:** a chapter processes unattended with no OOM kills and a parity report is
attached to the milestone. Open: the external `koharu-rs/torch` asset, a physical-device run,
and the recorded parity/benchmark numbers.

---

### M4 — Touch UI

**Goal:** import → edit → typeset → export, one-handed.

- [x] Mobile app bar + overflow menu; desktop shortcut labels (window chrome and updater are already hidden on mobile in M0)
      (`MobileAppBar.tsx`: title, Pages/Inspector/Activity toggles, Import/Export/Share/Undo/Redo/Settings/Close overflow; no accelerator labels)
- [x] Canvas-first layout; Pages/Inspector/Agent/Activity become bottom sheets
      (`packages/ui` already ships `sheet.tsx`, `drawer.tsx`, `sidebar.tsx`);
      horizontal bottom toolbar; compact status bar
      (`MobileSheets.tsx` opens `PageRail`/`Inspector`/`ActivityBody`; desktop keeps the 18/58/24 panes)
- [x] Gestures in `CanvasWorkspace.tsx`: pinch-zoom, two-finger pan, long-press, touch slop,
      guard against a second pointer; keep single-finger draw
      (touch path is `pointerType === 'touch'`-gated, so mouse/trackpad is unchanged)
- [x] Touch-sized targets (≥44 px hit area, visual size unchanged) in `SelectionControls.tsx`,
      `Inspector.tsx`, `ToolBar.tsx`; explicit multi-select mode instead of Shift/Ctrl;
      pointer-based page reorder; explicit rename
- [x] Keyboard-aware text editing (canvas overlay or bottom sheet); `dvh`; viewport metadata
      (Inspector opens as a bottom sheet; `interactive-widget=resizes-content` + `dvh`)
- [x] Hide the Agent panel on Android v1 (commands stay compiled)
- [x] Locale keys for every new string in all 11 locales (`menu.share`, `menu.more`,
      `navigator.multiSelect`, `navigator.reorder`)
- [x] Screenshots at 360×800, 412×915, 800×1280 (plus a long-label case)
      — captured 2026-10-22 on the foldable by overriding the display
      (`wm size <px>` + `wm density 480`, then `wm size reset`/`wm density reset`), which yields
      exact CSS viewports: `1080×2400` → 360×800, `1236×2745` → 412×915, `2400×3840` → 800×1280.
      All three render the app bar + model chip + Run, the WebGPU canvas, and the floating bottom
      toolbar; the project title in the app bar truncates with an ellipsis at every width, which
      is the long-label case. Remember `screencap -d <display-id>` on this foldable.

**Exit criteria:** a page can be imported, edited, typeset, and exported on a phone without
a keyboard, mouse, or hover.

Device progress: import, the mobile shell (app bar + Pages/Inspector/Activity sheets), the
canvas render, the Activity sheet's failure surface, and the layout at 360×800 / 412×915 /
800×1280 are verified on the phone. The edit → typeset → export pass is partly verified: a
full-resolution PNG export (1126 × 1600, named `0001_<page label>.png`) landed in
`Documents/Test/` through the SAF tree picker. **Typeset needs translation output, so it stays
blocked on the native runtimes.**

---

### M5 — Perf, thermal, device matrix

- [ ] Reference devices: 8 GB mid-range, 12 GB flagship, 6 GB floor
- [ ] Sustained 10-page run: thermal throttling, LMK behavior, battery drain
- [x] Battery-saver profile (fewer threads, smaller quant, lower concurrency)
      (`koharu-ml` `LoadProfile::power_saver` halves decode/prompt threads and
      batch/ubatch and trims the resident budget 25%; opt in with `[runtime]
      battery_saver = true`, resolved by `koharu-runtime` and applied by the local
      translator. Off by default; desktop is unchanged)
- [ ] Decide whether RF-DETR survives on mobile; publish the supported matrix
      (the docs matrix is a placeholder pending the reference-device runs)

---

### M6 — Release

- [x] CI Android job: JDK, SDK, NDK, `tauri android build`, AAB/APK artifacts, keystore
      signing, `versionCode` synced from release-please
      (a `release` job in `.github/workflows/android.yml`; signing via a Gradle init script
      and the workspace version via `cargo metadata` + `--config`. The preview `build` job
      publishes the debug APK on `android-m*` tags; the signed AAB job still needs the
      `ANDROID_KEYSTORE_*`/`SENTRY_*` secrets and has not run. Review fix 2026-10-22: the
      keystore password/alias/password were scoped to the "configure signing" step's `env:`
      only, so the later Gradle build step saw them empty — all four keys now go to
      `$GITHUB_ENV`)
- [ ] Play internal track, arm64 ABI split, native symbol upload (`.github/sentry.sh` has no
      Android branch)
      — ABI split and the sentry Android branch landed; the Play internal-track upload is
      manual (no publisher action is wired, per the no-new-third-party-actions rule)
- [x] Docs: Android tab in `installation`, `hardware`, `quickstart` across en/ja/zh
- [ ] Upstream discussion issue + split PR series (required by `CONTRIBUTING.md`)

---

## Risk register

| Risk | Impact | Mitigation | Where |
|---|---|---|---|
| WebGPU missing in Android **WebView**, or `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` absent | Canvas unusable | Resolved 2026-10-22 on hardware (Adreno 8xx): adapter present, `create_canvas` succeeds and renders, so the downlevel flag is there. No native SurfaceView present path or CPU-composited fallback needed | M0/closed |
| libtorch Android build, size, or operator coverage | M3 slips | Timebox a build spike first; fall back to an AAB feature module; selective-ops build only if size forces it | M3 |
| Gemma 4 E4B memory on 8 GB | OOM | RAM-class profiles, text-only translation, E2B below 8 GB | M2 |
| Sustained decode speed | Poor UX | Queue + foreground service, per-page progress, fast profile (Qwen 3.5 0.8B) | M2/M4 |
| SAF URI handling via plugin | Import/export broken | Resolved in M1: `tauri-plugin-dialog` returns `content://` URIs and cannot pick a directory, so `koharu-android` owns one SAF surface (import, tree export, create-document) and stages copies in app cache | M1 |
| Play policy on bundled native runtimes | Distribution | Bundle `.so` only; never download executable code | M0/M6 |
| Detection model contract mismatch | Pipeline quality | Two-way spike with parity comparison | M3 |
| `native-tls`/OpenSSL in the NDK build | Build failure | Switch to rustls first | M0 |
| `rustls-platform-verifier` (reached through `reqwest` and `hf-hub`) on Android | Every Hugging Face download hung with no error | Resolved: `hf-hub` (whose private `no_redirect_client` bypassed the injected TLS config) is dropped; downloads go over the shared client with bundled Mozilla roots via `configure_reqwest_tls` | M4 |
| Bundled runtime missing at model load | Pipeline stalls instead of erroring | Resolved 2026-10-22: `koharu_ml::features()` exposes the runtimes that activated; every stage load fails with an error naming the runtime it needs before the lazy `-sys` loader can panic, and the `process` task catches a residual panic so the job always reaches a terminal `Failed` state | M4/closed |
| Work that finishes within milliseconds killed the app | `Run` on a job that fails fast (missing runtime, bad input) crashed the process with `ForegroundServiceDidNotStartInTimeException` | Fixed 2026-10-22: the stop is handed to the service and applied only after it is foregrounded, so a stop that overtakes its start can no longer satisfy the `startForegroundService` deadline | M1/closed |

---

## Verification protocol

From `AGENTS.md`, applied per milestone:

- Development uses the smallest relevant check once, debug profile. No full suites, no
  repeated unchanged runs, no release-profile builds unless explicitly requested.
- Every UI state change needs a screenshot; truncation bugs need a long-label case.
- Native runtime work needs a structured comparison on identical inputs, not just a green build.
- Model-port work reports device, input size, baseline, result, and correctness delta.
- End-to-end tests only when explicitly asked for.
- PR text stays short: problem, change, verification.

---

## Debugging on a device

The dev host is a QEMU VM with no USB passthrough, so the phone is reached over TCP (run
`adb tcpip 5555` once over USB from a host that has the phone, then accept the debug prompt):

```sh
export PATH="$HOME/Android/Sdk/platform-tools:$PATH"
adb connect <phone-ip>:5555
```

Local debug APK build (the CI preview additionally sets `CARGO_PROFILE_DEV_DEBUG=0`):

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
export JAVA_HOME="$HOME/.local/opt/jdk-21.0.12.1+1"
cd crates/koharu
CARGO_PROFILE_DEV_DEBUG=0 cargo tauri android build --debug --target aarch64 --apk
adb uninstall rs.koharu.app   # the local APK uses a different debug key than CI
adb install -d gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

Signals to reach for, in order:

- **App log (Rust `tracing`):** `adb exec-out run-as rs.koharu.app cat logs/koharu.log`.
  `tauri-plugin-log` is configured with `clear_targets()` and a single `LogDir` target, so
  this file is the only Rust output; `adb logcat` has none. `store:` lines trace the runtime
  store's lock → stage → publish transitions and are the fastest way to localise a download.
- **`adb logcat` is not readable on the test phone.** Its vendor privacy wrapper
  ("HKS") replaces every line with an opaque token (`ring buffer is … 0 B readable`), including
  the `crash` buffer. Get Java-level failures from `adb shell dumpsys dropbox --print`
  instead: it carries the process, the exception, the full stack, `Lifetime`, and the
  `Crash-Handler`. `adb shell dumpsys activity services` shows the live `ServiceRecord` state
  (`startRequested`, `isForeground`, `startForegroundCount`) while the process is up. This is
  how the foreground-service start/stop race was found.
- **Screenshots need an explicit display on this foldable.** `adb exec-out screencap -p`
  grabs the first display, which is the folded inner panel and reports `state OFF`, so the
  image is black. List the ids with `adb shell dumpsys SurfaceFlinger --display-id` and capture
  the active one: `adb shell screencap -d 4630946324137792660 -p /sdcard/shot.png` then
  `adb pull`. The app renders on the 1060 × 2376 display.
- **Runtime store:** `adb shell run-as rs.koharu.app ls -R runtime` — files live at
  `runtime/hugging-face/{models,datasets}/<repo>/snapshots/<revision>/<file>` with a
  `.<file>.lock` beside each. A `.lock` with no matching file means the download never
  published.
- **Frontend / WebView:** debug builds expose the WebView devtools socket. Forward it and
  drive the page with CDP `Runtime.evaluate`:
  ```sh
  PID=$(adb shell pidof rs.koharu.app)
  adb forward tcp:9222 localabstract:webview_devtools_remote_$PID
  curl -s http://127.0.0.1:9222/json    # take webSocketDebuggerUrl
  ```
  This is how the import + Run flow was driven without touching the screen. Caveat: the page
  blocks cross-origin `fetch` (CSP/CORS), so it cannot verify the app's own network path;
  use a probe binary (below) for that.
- **Standalone probe:** cross-compile a small program that mirrors the app's client
  configuration and push it, to test the network independently of the app:
  ```sh
  cargo ndk -t arm64-v8a --platform 31 build --release
  adb push target/aarch64-linux-android/release/<bin> /data/local/tmp/
  adb shell 'chmod 755 /data/local/tmp/<bin> && /data/local/tmp/<bin>'
  ```
  Pass `--platform 31` to match the app's `minSdk`; a lower default can fail to link (e.g.
  `getifaddrs`, which needs API ≥ 24).
- **Not available:** `debuggerd` and `/proc/locks` need root and production builds reject
  `adb root`; `/proc/<pid>/task/*/wchan` reads back `0`. Work from the app log and store state.

Click through the Export Project submenu for the remaining codecs: PNG uses the tree picker
(one file per page, written as `0001_<page label>.png`), CBZ uses create-document with the
project name pre-filled. Drive the system picker with `uiautomator dump` — its XML gives exact
bounds, and the dump's coordinates are the panel's, so a row hidden under the toolbar looks
tappable but is not. The root of internal storage and `Download` are restricted roots on
Android 11+ ("Can't use this folder"); pick `Documents`, then `USE THIS FOLDER`, then `ALLOW`.

Device facts for the mobile profile (test phone, Android 16): `MemTotal` 15,601,536 kB
≈ 14.9 GiB → `RamClass::High` (5.5 GiB translation budget), so Gemma 4 E4B fits and is the
right default (`Q2_K_XL` 3.22 GB for headroom, `Q4_K_XL` 4.22 GB also inside budget). The
phone pulled 52 MB from Hugging Face in 2.1 s (~24 MB/s), so on-device model downloads are
practical. Page size is 4 KB, so 16 KB library alignment is not exercised by this phone.

---

## Building the Android runtimes

Neither runtime is buildable from this repository (`koharu-llama-sys` and
`koharu-torch-sys` vendor headers only), and neither upstream publishes an
Android asset. A working device build therefore needs both produced locally
first. The scripts live at `$HOME/build-android/` on the dev host:
`build.sh` (llama), `configure.sh` + `shim/` (torch), and the PyTorch source
patch as `koharu-pytorch-android.patch`.

**llama.cpp** — the `koharu-rs/llama` CMake wrapper around upstream
`ggml-org/llama.cpp` at tag **`b11642`** (byte-identical to the headers in
`crates/koharu-llama-sys/include`, so the version is not optional). The wrapper
only accepts CUDA/HIP/VULKAN/METAL, so it needs a local patch adding a `CPU`
backend (dedupe `ggml-cpu` in the WHOLE_ARCHIVE target list). Configure with the
NDK toolchain, `-DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-31`,
`GGML_OPENMP=OFF`, `GGML_LLAMAFILE=OFF`, `GGML_CPU_KLEIDIAI=ON`, then build the
`llama mtmd` targets. Yields `libllama.so` (55 MB) + `libmtmd.so` (21 MB) with
only libc/libm/libdl/liblog in `DT_NEEDED` — the ggml static targets are folded
into `libllama`, which is what the two-name prop set assumes. Note KleidiAI does
nothing for the Q2_K_XL quant: the device logs `no kernel for tensor type tq2_0,
not accelerated by KleidiAI`.

**libtorch** — PyTorch `v2.13.0` cross-compiled for Android. Android is
normally a *mobile* build, and `c10/macros/Macros.h` defines `C10_MOBILE` from
`__ANDROID__` regardless of CMake, which (a) drops the C++ frontend and autograd
the tch shim links against and (b) localizes symbols. The build therefore sets
`KOHARU_FULL_LIBTORCH` to opt out of the CMake mobile path and needs four source
patches, all in `koharu-pytorch-android.patch`:

1. `CMakeLists.txt` — gate `INTERN_BUILD_MOBILE` with `AND NOT
   KOHARU_FULL_LIBTORCH`, so `NO_API`/autograd stay on.
2. `cmake/Dependencies.cmake` — put `ANDROID` in the pthreadpool gate
   (`C10_MOBILE` is still defined, so `ATen/ParallelNative.cpp` needs
   `caffe2::pthreadpool`).
3. `cmake/Dependencies.cmake` — link the full `onnx` library on Android when
   `KOHARU_FULL_LIBTORCH`; the default Android path links only `onnx_proto`,
   but the non-mobile JIT serialization sources call ONNX's checker and shape
   inference. (Their headers also have to be on the include path; copying
   `third_party/onnx/onnx/*.h` into the build tree does it.)
4. `aten/src/ATen/DynamicLibrary.cpp` — compile its implementation even under
   `C10_MOBILE`, which the JIT CPU fuser needs.
5. `third_party/mimalloc/CMakeLists.txt` — drop `-ftls-model=initial-exec` on
   Android. mimalloc is linked into `libc10.so` and bionic refuses an
   initial-exec TLS relocation in a `dlopen`ed library
   (`TLS symbol "(null)" … using IE access model`), which is exactly how the
   app loads its bundled runtimes. Check with
   `readelf -rW libc10.so | grep -c TLS_TPREL64` — it must be 0.
6. `caffe2/CMakeLists.txt` — compile `aten/src/ATen/core/dynamic_type.cpp` with
   `-fvisibility=default`. The JIT IR header instantiates
   `c10::DynamicType::fallback()` from a template the shim uses, and the global
   `-fvisibility=hidden` keeps it out of `libtorch_cpu.so`'s dynamic symbol
   table, so the shim fails to load (`cannot locate symbol
   _ZNK3c1011DynamicType8fallbackEv`).

To check the shim will load before spending an APK build on it, diff its
undefined symbols against everything the runtime libraries export:

```sh
nm -D --undefined-only libkoharu-torch.so | awk '{print $1}' | sort -u > /tmp/u
cat libc10.so libtorch_cpu.so libtorch.so libtorch_global_deps.so \
  | nm -D --defined-only - | awk '{print $1}' | sort -u > /tmp/d
comm -23 /tmp/u /tmp/d | grep -v '@LIBC'
```

Anything left other than the weak `_ZTH14torch_last_err` (the shim's
`torch_last_err` TLS init, which bionic tolerates unresolved) will fail at
`dlopen`.

Other required configuration: host `protoc` via
`-DCAFFE2_CUSTOM_PROTOC_EXECUTABLE` (a cross build cannot run its own),
`-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON`, `-DUSE_OPENMP=OFF` (the props list
no OpenMP runtime), `-DUSE_KLEIDIAI=OFF`, Eigen under `third_party/eigen`, and
`-DBLAS=Eigen -DUSE_MKLDNN=OFF -DUSE_FBGEMM=OFF -DUSE_XNNPACK=OFF
-DUSE_PYTORCH_QNNPACK=OFF`. Expect ~1-2 h and ~20 GB; the `libtorch_cpu.so` link
is the peak. Result: `libtorch_cpu.so` 215 MB, `libc10.so`, `libtorch.so`,
`libtorch_global_deps.so`, all 16 KB-aligned.

**The shim** (`koharu-rs/torch`) then builds against the install tree with
`find_package(Torch)`. Two Android-specific details: the NDK toolchain restricts
config-mode package search to the sysroot, so pass `-DTorch_DIR=<install>/share/cmake/Torch`
(and `CMAKE_FIND_ROOT_PATH_MODE_* = BOTH`); and the NDK links shared objects with
`-z defs` while desktop allows undefined symbols, so `-DCMAKE_SHARED_LINKER_FLAGS="-Wl,-z,undefs"`
is required to match desktop's lazy-resolution behaviour. Yields
`libkoharu-torch.so` (18 MB, 2734 exported `at_*`/`atg_*` symbols).

Finally `scripts/bundle-android-runtimes.sh <llama-dir> <torch-dir>` copies all
seven libraries plus the NDK's `libc++_shared.so` into `jniLibs/arm64-v8a/`.
The five torch names and the two llama names must stay identical to the
`android` props in `runtime/packages/{torch,llama}.rs`.

---

## Prerequisites

The dev host now has the toolchain:

| Tool | Required | Present |
|---|---|---|
| Rust + `rustup` | 1.97.1+ (edition 2024) | ✅ 1.99.1 |
| Rust targets | `aarch64-linux-android`, `wasm32-unknown-unknown` | ✅ |
| Bun | 1.3.14+ | ✅ 1.4.3 |
| Node | for tooling | ✅ v24.15.0 |
| JDK | 21 | ✅ `~/.local/opt/jdk-21.0.12.1+1` |
| Android SDK | platform 36, build-tools, cmdline-tools | ✅ (AGP also pulls `android-37.0` for `compileSdk 37`) |
| Android NDK | r27c | ✅ 27.2.12479018 |
| `cargo-ndk` | 4.1.2 (`--locked`) | ✅ 4.1.2 |
| Tauri CLI | installed by root `postinstall` at the pinned rev | ✅ 3.0.0-alpha.5 |
| Docker | optional for native builds | ✅ |

```sh
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-linux-android wasm32-unknown-unknown
cargo install cargo-ndk --version 4.1.2 --locked

# Bun
curl -fsSL https://bun.sh/install | bash

# Android (paths used on this host; set these in your shell profile)
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
export JAVA_HOME="$HOME/.local/opt/jdk-21.0.12.1+1"
sdkmanager "platforms;android-36" "build-tools;36.0.0" "ndk;27.2.12479018"

# App deps (also installs the Tauri CLI)
bun install
```

The Tauri CLI does not search for a JDK: it only auto-detects Android Studio's JBR at
`/opt/android-studio/jbr` or `java` on `PATH`. On hosts with neither, export `JAVA_HOME` to
a JDK (21 works).

Device testing: `adb reverse tcp:3000 tcp:3000` is only needed for `tauri android dev` with
the Next dev server. The built debug APK embeds the frontend and serves
`http://tauri.localhost/`; it needs neither `adb reverse` nor a running dev server. For the
dev server over the network, set `TAURI_DEV_HOST` to the host LAN address instead.

---

## Open questions

- Detection: wiring landed for all three detectors; the mobile default is YOLO26s. The final
  quality-vs-latency pick still needs the reference-device numbers (M2/M5).
- Whether to ship Vulkan at all on Android, or CPU-only for v1 (decided in M2 benchmarks).
- APK size budget if libtorch cannot be trimmed: AAB feature module vs. Play asset delivery.
  Still open until the `koharu-rs/torch` Android asset exists.
- OCR language selection: the mobile default is Manga OCR (Japanese). Auto-selecting
  MangaOCR vs. PaddleOCR-VL GGUF per page language is not implemented.
- A missing bundled runtime: resolved 2026-10-22. A stage whose runtime did not activate now
  fails the job with a readable error (`<model> needs the Torch runtime, which is not
  available on this device`) instead of stalling, and the job task catches a residual panic so
  the job cannot stay `Running` forever.
- Android TLS trust: `configure_reqwest_tls` uses bundled Mozilla roots, so user-installed and
  corporate CAs are ignored. Revisit only if a user needs them — that would mean initializing
  `rustls-platform-verifier` over JNI with the `rustls-platform-verifier-android` component.

---

## Session log

Append one entry per working session: date, milestone, what changed, what was verified,
and what the next session should pick up.

| Date | Milestone | Summary | Next |
|---|---|---|---|
| 2026-10-21 | Plan | Repository surveyed; blockers, model profile, and milestones defined. No code changes. | Install toolchain, create `android` branch, start M0 |
| 2026-10-21 | M0 | Installed toolchain and host deps. `koharu-app` now builds on `DynRuntime` (CEF/Wry chosen by the entry), plugins and dialogs split by target, Android paths injected, ML init lazy, secrets fail soft on Android, `koharu-bindgen` passes the NDK sysroot. `cargo ndk check` and `tauri android build --debug --target aarch64` succeed; APK is `rs.koharu.app`, `minSdk 31`, `arm64-v8a`; desktop `cargo check` still passes. | Install the APK on a device/emulator, verify the shell leaves `StartupView`, run the WebGPU probe, then exercise the CI workflow |
| 2026-10-21 | M0 fixes | `process` loads the pipeline before registering the stop token/job, so a failed model load no longer leaves a phantom running job or blocks later requests with "another process is already running"; `koharu_config::configure` rejects configuration after the manager was created and `path()` is documented with the override; the unused workspace `open` dependency was removed; the desktop Settings panel keeps a square top-right corner (`md:rounded-tr-none`); the mobile startup screen hides desktop window chrome and respects safe-area insets. Verified on the emulator (Android 15, `sdk_gphone64_x86_64`): fresh 1.6 GB debug APK installs and boots after `adb uninstall rs.koharu.app` (`INSTALL_FAILED_INSUFFICIENT_STORAGE` otherwise), StartView and Settings render, storage hint and desktop title bar gone on mobile, Settings uses the stacked mobile layout with safe-area padding, assets come from `http://tauri.localhost/` with no dev server. WebGPU probe: `navigator.gpu` present, `requestAdapter()` returns `null` (SwiftShader), `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` unchecked. No physical phone tested. Hosts without Android Studio must export `JAVA_HOME` (21 works) — the Tauri CLI only auto-detects `/opt/android-studio/jbr` or `java` on `PATH`. `cargo-ndk` 4.1.2 is the tested version, now pinned in `android.yml`; CI has not run. | Install and boot on a physical phone, rerun the WebGPU probe on hardware, get the first CI run green |
| 2026-10-21 | M0 review | Fixed `clippy::redundant_closure` in `koharu-bindgen`. Hid the Providers tab on mobile until the M1 Keystore store: a typed credential made "Back to editor" fail forever. Corrected the plan's stale Settings, startup, and external-link rows, the M4 Settings/safe-area overlap, the `adb reverse` note, and the desktop-error note (command errors are already toasted by `call()` in `packages/koharu/lib/backend.ts`). Added tag-triggered preview releases (`android-m0-*`, `CARGO_PROFILE_DEV_DEBUG=0`) and made the desktop Release workflow ignore `android-*` tags. CI run not verified yet. | Rerun CI, install and boot on a physical phone, rerun the WebGPU probe on hardware |
| 2026-10-21 | M1 | Added the `koharu-android` Tauri mobile plugin (SAF import/tree/create-document, share sheet, Keystore AES-GCM store, foreground service + partial wakelock, background command) and registered it from `crates/koharu`; `koharu-app` gained the picker/`Destination` abstraction with app-cache staging, `koharu-secrets` gained an installable `Backend`, and zero-prose notification payloads; the frontend gained `lib/back.ts`, re-enabled the Providers tab, and shows a platform-specific storage note in all 11 locales. `tauri-plugin-dialog` was rejected as the picker (content URIs, no tree picker) and the risk is closed. Verified: `cargo check -p koharu`, `cargo ndk -t arm64-v8a check -p koharu`, `cargo tauri android build --debug --target aarch64`, `:koharu-android:compileDebugKotlin`, oxlint + tsc; the APK installs and boots to StartView on the API 35 emulator. | Exercise the exit criteria on a device (import a CBZ, export a PNG, save a key, run a job with the screen off), then start M2 |
| 2026-10-21 | M1 review fixes | Corrected the review findings: the import picker no longer MIME-filters (it hid `.cbz`/`.rar`); an undecryptable Keystore entry is dropped and reported unset instead of failing `Preferences::load`; `POST_NOTIFICATIONS` is requested at the first foreground start; the foreground refcount lock spans the count and the start/stop send, removing the stop-after-start race; notification text is data only and its title falls back to the application label; the back registry closes Settings with a save and the delete confirmation before backgrounding, and uses `dispatch`; non-Android mobile compiles again with the pre-M1 "picker unavailable" arms. Also removed the stale plugin `settings.gradle`, the unused `serde_json` dependency, and made the service instance `@Volatile`. | Same as M1: device verification, then M2 |
| 2026-10-21 | M2 code | `koharu-runtime`: the Android probe always exposes a CPU device (memory from `/proc/meminfo`) and a Vulkan device only when the `runtime` config section sets `android_vulkan`; `Llama::AndroidCpu`/`AndroidVulkan` are bundled (no download, dlopen by soname); `Runtime::discover` now skips requested features with no package and exposes `Runtime::features()`; bundled loader falls back to the bare soname. `koharu-bindgen`: the `RTLD_NOLOAD` already-loaded arm covers `android`, and the three `-sys` crates declare `libc` for Android. `koharu-ml`: `llm::profile` owns the RAM classes/budgets, the mobile load profile (mmap, q8 KV, performance-core threads, capped context/tokens) and a budget guard; `koharu-ml::init` initializes only the backends that resolved. `koharu-translator`: the local translator applies the mobile profile; a new `bench` bin reports tok/s, page latency, peak RSS, and thermal status. Docs: Android section in `hardware.mdx` (en/ja/zh). Verified: `cargo check` for the four crates, `cargo ndk -t arm64-v8a check -p koharu`, and the focused `koharu-ml` `llm::` tests (10 pass). | Publish the `aarch64-linux-android` llama libs, install on a physical phone, run the WebGPU probe and the `bench` harness, then fill the hardware table |
| 2026-10-21 | M2 review fixes | The Vulkan setting is now meaningful: the loader no longer forces `gpu_layers = 0`, so the CPU path runs zero layers and the opt-in Vulkan device offloads. The forced `n_ctx`/`max_tokens` caps were removed (context derives from the prompt) so long pages no longer hard-fail or truncate; the performance-core thread fallback no longer uses every core; the budget guard now counts the mmproj projector; the benchmark harness applies the mobile profile; `loader::load` only falls back to a bare soname on Android; discovery logs skipped features. Docs: `hardware.mdx` (en/ja/zh) now states the milestone covers translation and documents the `[runtime] android_vulkan` key, and the M2 profile item matches the loader. Verified: focused `koharu-ml` `llm::` tests and `cargo check`. | Publish the Android llama libs and verify on a physical phone |
| 2026-10-22 | M3 review fixes | `koharu-runtime`: `Plan::initialize` now activates every reachable package in dependency order and skips (with a warning) any that fail to load, so a missing or mismatched bundled runtime can no longer abort the runtimes that did load — this removes the coupling where a missing `libtorch` took the llama/translation backend down with it. `Runtime::features()` now reports the packages that actually activated, and `koharu-ml::init` reads it after `initialize()`. `parity` bin: dropped the always-null `bubble-mask` field and the run-specific `revision` field, and pins greedy decoding (`temperature 0`, `top_k 1`) so the report is diffable across devices. Plan: reconciled the mobile profile table with the single platform default (YOLO26s / Manga OCR / AOT), fixed the `cargo ndk` typo, and noted the bundled Torch `DT_NEEDED`/OpenMP caveat. Verified: `cargo check -p koharu-app`, `cargo test -p koharu-runtime -p koharu-pipeline --lib`, `cargo ndk check -p koharu`. | Build and publish the preview APK, then run the parity/bench harness on a device |
| 2026-10-22 | M3 code | `koharu-runtime`: `Torch::AndroidCpu` is a bundled package (bare sonames, empty dependencies) and `discover()` maps Android to CPU (Vulkan→CPU until validated); `koharu-ml/src/lib.rs` comment updated. `koharu-pipeline`: the detection stage now normalizes every detector onto a pipeline-owned `Detection`/`DetectionMask`/`Detections` record — RF-DETR is copied, YOLO26s maps `balloon`/`frame`/`text` → bubble/panel/text, PP-DocLayoutV3 rasterizes its polygon masks and keeps only text regions; `DetectionModel`/`OcrModel`/`InpaintingModel` gained platform-aware defaults (Android: YOLO26s, Manga OCR, AOT; desktop unchanged) with new `PpDocLayoutV3Config`/`ComicLayoutYolo26sConfig` profiles and processor keys; `AcceleratorGate` takes a `MemoryPolicy` — mobile serializes the lane and evicts other stages before every load, and `recover()` no longer skips the CPU lane; the `run` bin gained the two new detection choices and a new `parity` bin dumps boxes/mask digests/OCR/translations to JSON. `koharu-app` remembers the new detection profiles. Frontend: the detection picker gained both models with a threshold field, `packages/bridge/src/protocol.ts` regenerated, and the `threshold` label added to all 11 locales. Docs: `hardware.mdx` (en/ja/zh) describes the mobile defaults, one-resident-stage policy, bundled Torch runtime, and the parity harness. Verified: `cargo check -p koharu-app`, `cargo test -p koharu-pipeline --lib` (53 pass), `cargo ndk -t arm64-v8a check -p koharu` (green; pre-existing Android VRAM dead-code warnings only), oxlint, `tsc --noEmit`, and the editor component tests (50 pass). Not verified: no device run and no `koharu-rs/torch` Android asset yet. | Publish the `aarch64-linux-android` torch libs, run the pipeline and `parity`/`bench` on a physical phone, attach the parity report, then start M4 |
| 2026-10-22 | M6 release config | Added a `release` job to `.github/workflows/android.yml` (the preview job keeps its behavior, gated to branch/PR/`android-m*` refs): JDK 21 + Android SDK + NDK r27c, reads the workspace version with `cargo metadata` and passes it as `--config '{"version":…}'` so Tauri derives `versionName`/`versionCode` from the release-please bump (the Android generator otherwise fell back to `1`/`1.0`), builds `--target aarch64 --split-per-abi`, injects the release keystore through a `$HOME/.gradle/init.d` init script (no hand-edit of `gen/android`), and uploads the AAB + APK artifacts. Added an `android` branch to `.github/sentry.sh` (invoked as `bash .github/sentry.sh android`) that uploads the unstripped `jniLibs/arm64-v8a/libkoharu*.so`, `libllama*.so`, `libmtmd.so`, and `libtorch*.so`; the macOS/Windows/Linux branches are unchanged. Docs: Android tab + supported-device matrix in `installation.mdx` (en/ja/zh), Android project/import/export note in `quickstart.mdx` (en/ja/zh), and a pending-numbers note in `hardware.mdx` (en/ja/zh). Not verified: CI has not run; keystore secrets, a Play service account, and the reference devices are still needed. Play internal-track upload is manual (no publisher action is wired). | Configure the `ANDROID_KEYSTORE_*`/`SENTRY_*` secrets, add the `gh workflow run android.yml --ref "$TAG"` dispatch to `release-pr.yml`, run the workflow on a release tag, and publish the AAB to the Play internal track |
| 2026-10-22 | M4 + M5 code | M4 touch UI: `CanvasWorkspace` gained a `pointerType === 'touch'` gesture path (8 px slop so a tap cannot paint, long-press selection, pinch-zoom + two-finger pan reusing `clampCameraTranslation`, second/third-pointer guards) and mouse/trackpad is byte-for-byte unchanged; `SelectionControls` handles have 44 px transparent hit boxes with the visual knob unchanged; `ToolBar`/`StatusBar` become a horizontal bottom toolbar and compact bar on mobile. Mobile shell: `Editor` branches on platform (`MobileEditor` = app bar + canvas + bottom sheets), new `MobileAppBar` (Pages/Inspector/Activity toggles plus Import/Export/Share/Undo/Redo/Settings/Close overflow) and `MobileSheets` (Pages/Inspector/Activity as `Sheet side='bottom'`, Android back closes the open sheet), `store.mobilePanel`, `ActivityBody`/`useActivityCount` extracted, `dvh` + a Next `viewport` export with `interactive-widget=resizes-content`, Agent hidden on mobile. Panels: `PageRail`/`Inspector`/preferences get ≥44 px targets, an explicit page multi-select toggle, pointer-based reorder (`elementFromPoint` → `commands.movePage`), always-visible actions and menu rename; desktop Shift/Ctrl/range and HTML5 drag are kept. New `share` typed command in `koharu-app` renders a page to app cache and calls `Android::share`; `packages/bridge/src/protocol.ts` regenerated. Locales: `menu.share`, `menu.more`, `navigator.multiSelect`, `navigator.reorder` added to all 11. M5: `LoadProfile::power_saver` in `koharu-ml` halves decode/prompt threads and batch/ubatch and trims the budget 25%; the `[runtime] battery_saver` key is owned and resolved by `koharu-runtime` (`battery_saver_enabled()`) and applied by the local translator. Off by default; desktop is unchanged. Verified: `tsc --noEmit` clean, oxlint clean (one pre-existing `clamp` warning in `ToolBar.tsx`), focused vitest `editor-components`/`workspace`/`canvas`/`localization` (78 pass), `cargo check -p koharu-app -p koharu-ml -p koharu-translator -p koharu-runtime`, focused `koharu-ml llm::` tests (12 pass), and `cargo ndk -t arm64-v8a check -p koharu` (green; pre-existing VRAM dead-code warnings only). Not verified: no device/emulator run and no screenshots at 360×800/412×915/800×1280. | Run the M4 exit criteria and capture screenshots on the emulator/phone; publish the `koharu-rs/{llama,torch}` Android assets; configure release secrets and run the CI release job |
| 2026-10-22 | M4/M5/M6 review fixes | Applied the code-review findings. Rust: `cargo fmt` across the workspace (the modified `output.rs`/`lib.rs`/`local/mod.rs` plus three pre-existing unformatted files — `koharu-ml/llm/model.rs`, `koharu-runtime/runtime/packages/llama.rs`, `koharu-translator/bin/bench.rs`) so `cargo fmt -- --check` is green. Frontend: removed the unreachable `platform === 'mobile'` early return in `RightSidebar` (only mounted on desktop), deleted the dead `clamp` helper in `ToolBar`, and spaced the mobile rotate handle + raised corner-handle hit priority in `SelectionControls` so the 44 px boxes stop overlapping on small selections (desktop unchanged). Docs: documented the `[runtime] battery_saver` key in `hardware.mdx` (en/ja/zh) and dropped the outbound "share sheet" mention from the Android import note in `quickstart.mdx` (en/ja/zh). Plan: clarified the M6 CI note to distinguish the preview debug APK from the unrun signed AAB job. Verified: `cargo fmt --check`, `tsc --noEmit`, oxlint, focused vitest (editor-components + workspace), `cargo check`, `cargo ndk -t arm64-v8a check -p koharu`. Note on the earlier "importing files is not supported" report: that string came from the M0 preview, which shipped the `#[cfg(mobile)]` picker stub; M1 replaced it with the SAF `pickImport` implementation (`KoharuPlugin.kt`), so the current build imports through the system picker. | Test the preview APK on a physical phone; capture the M4 screenshots; publish the `koharu-rs/{llama,torch}` assets; configure the release secrets |
| 2026-10-22 | M4 device fix | Debugged the "download stuck on `config.json`" report on a physical phone (Android 16, arm64). Cause: `reqwest`'s `rustls` backend verifies through `rustls-platform-verifier`, which on Android must be initialized over JNI against the host activity; without that it panics at `android.rs:90` on the first handshake. Because the model download runs in a detached task, the panic unwound the task silently — the `Started` download event had no terminal event, so the activity center showed "downloading config.json" forever and the runtime store kept only a stale `.config.json.lock`. Verified with a standalone on-device probe against the app's exact client config. Fix, two parts. (1) `koharu-runtime::configure_reqwest_tls` builds a `rustls::ClientConfig` from the bundled Mozilla roots (`webpki-roots`) on Android (platform verifier kept on desktop) and is used by `network::http()` and the translator's provider client. (2) `HuggingFaceFile` no longer uses `hf-hub`: `HFClient::build` always creates an internal `no_redirect_client` with default reqwest TLS settings and runs the download probe through it, so it bypasses the caller's configuration and panics on Android regardless of the injected client. Downloads now hit `/{repo}/resolve/{revision}/{file}` (and the `datasets/` variant) over the shared client, and `hf-hub` is dropped from the workspace. Trade-off: Android no longer trusts user-installed CAs. | Verify a full detection/OCR/translation run on the phone; publish the `koharu-rs/{llama,torch}` assets so the stages can load |
| 2026-10-22 | M4 device verification + release | Installed the preview on a physical phone (Android 16, arm64, minSdk 31) and drove the whole flow over adb + WebView CDP: create a project, import a real manga JPG through the system picker, pick a model, Run. The import picker works (the M0 stub is gone), and after the download fix the model files land — `config.json` (353 B) and `model.safetensors` (44 MB) in `runtime/hugging-face/models/mayocream--comic-layout-yolo26s/snapshots/<rev>/`, with `store: staged ok=true`. The run then stalls at the detection load because the bundled runtimes are absent (`Torch android-cpu` / `llama android-cpu` failed to activate) and no job failure is recorded — see [Open questions](#open-questions). Committed `c2c60237`, pushed, tagged `android-m4-preview-2`; CI green; the prerelease carries `app-universal-debug.apk`. Added a [Debugging on a device](#debugging-on-a-device) section (adb-over-TCP, the app log file, WebView CDP, the store layout, and the standalone-probe recipe). | Bundle `koharu-rs/{llama,torch}` into `jniLibs`, re-run the pipeline on the phone, decide hang-vs-error for a missing runtime, then the M4 screenshots and the M1 exit criteria |
| 2026-10-22 | Failure mode + native-runtime prep + device pass | Fixed the missing-runtime stall. `koharu-ml` now exposes `features()` (set from `runtime.features()` inside `init`), and `koharu-pipeline` guards each stage load with `require_runtime(model, feature)` so a stage whose runtime did not activate fails with `<model> needs the <feature> runtime, which is not available on this device` before the lazy `-sys` loader can `panic!`; `processing.rs` wraps `pipeline.execute` in `catch_unwind` so a residual panic still releases the stop token and publishes a terminal `Failed` job. Found and fixed a second app-killing bug on the device: work that finishes within milliseconds crashed the process with `ForegroundServiceDidNotStartInTimeException`, because `Guard::drop` sent Stop while `startForegroundService` was still in flight, tearing the service down before `onStartCommand` reached `startForeground`. `ForegroundService` now hands the stop to the service and applies it only after it is foregrounded (`startPending`/`stopPending`, and every intent is foregrounded before its action is interpreted). Native runtimes: reconciled the `android` props against `readelf -d` of the published Linux `v2.13.0.7` CPU asset (`libtorch_cpu.so` needs `libc10` + an OpenMP runtime; the NDK's is `libomp.so`, so the Android build must pass `-DUSE_OPENMP=OFF`; `libtorch_global_deps.so` is a dlopen root only). `graph.rs` now logs the full cause chain, `Torch::activate` names the library that failed, and `scripts/bundle-android-runtimes.sh` copies the seven libraries into `jniLibs`. M6: the release job's keystore password/alias/password were scoped to the "configure signing" step, so Gradle saw them empty — all four keys now go to `$GITHUB_ENV`. Also fixed the pre-existing `clippy::double_must_use` error in `koharu-runtime::lib.rs` that kept `cargo clippy --workspace -- -D warnings` red. Verified: `cargo fmt -- --check`, `cargo clippy --workspace -- -D warnings`, `cargo ndk -t arm64-v8a check -p koharu`, `cargo test -p koharu-pipeline --lib` (54 pass), the new `android.yml` parses, `bash -n .github/sentry.sh`. On the phone (test phone, Android 16, arm64, 1060 × 2376): installed the debug APK over the preview, imported project opens, and `Run` now fails fast with the readable error shown in the Activity sheet (`/tmp/shot3.png`) in 5/5 runs with no crash, where 3 of 4 runs previously killed the process. Not verified: no native runtime exists to bundle — `koharu-rs/{torch,llama}` publish no Android asset and this repository vendors headers only — so no stage can load and no M2/M3 numbers exist; the WebGPU probe, export/keys/long-job and the M4 screenshots remain open. Documented two device quirks in [Debugging on a device](#debugging-on-a-device): `adb logcat` is redacted on this phone (use `dumpsys dropbox --print`), and `screencap` needs `-d <display-id>` on the foldable. | Produce the Android runtime assets and run `scripts/bundle-android-runtimes.sh`, then run detection → OCR → translation → inpainting on the phone; capture the M4 screenshots; then the M1 exit criteria |

# Koharu Android — Build Plan

Working document for porting Koharu to Android. Written to be picked up cold in a new
session: each milestone below is self-contained, lists the files it touches, and defines
what "done" means.

- **Repository:** `$HOME/Koharu/koharu`
- **Baseline:** `main` @ `88d07ef0`, workspace version `0.83.5`, Tauri `3.0.0-alpha.5`
  (git rev `40a2d81d97f5c950e892fa7a6ce8b03064410c4d`), CEF runtime on desktop.
- **Status date:** 2026-10-21
- **Current state:** M0 build verification is done on the emulator — the debug APK builds,
  installs, boots, and renders the editor shell (StartView and Settings), so the `android`
  branch carries working code. Preview builds are tag-driven: pushing an `android-m0-*` tag
  makes CI publish the debug APK as a GitHub prerelease (no CI run has happened yet).
  Open: install and boot on a physical phone, rerun the WebGPU probe on real hardware.
  M1 and later are not started.

## How to use this document

| Session | Milestone | Start here |
|---|---|---|
| 1 | Toolchain + M0 | [Prerequisites](#prerequisites), [M0](#m0--it-builds-and-boots) |
| 2 | M1 | [M1](#m1--platform-services) |
| 3 | M2 | [M2](#m2--llamacpp-engine) |
| 4 | M3 | [M3](#m3--libtorch-full-pipeline) |
| 5 | M4 | [M4](#m4--touch-ui) |
| 6 | M5–M6 | [M5](#m5--perf-thermal-device-matrix), [M6](#m6--release) |

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
| Settings | `components/preferences/SettingsPage.tsx`, `PreferenceFields.tsx` | M0 shell is responsive: the sidebar collapses to a stacked nav with a three-column tab strip, `PreferenceRow` is one column below `md`, and safe-area insets apply. The remaining gap is touch polish (target sizes, keyboard handling), which is M4, not the layout. Providers is hidden on mobile until the Keystore-backed secret store lands in M1: Android credential writes fail and trap the Back-to-editor save. |
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
| 6–8 GB | Gemma 4 E2B Q2_K_XL (2.19 GB) | PP-DocLayout / YOLO26s | PP-OCRv6 rec (0.08 GB) / MangaOCR (0.46 GB) | AOT (0.02 GB) | ~3.5 GB |
| 8–12 GB | **Gemma 4 E4B Q2_K_XL (3.22 GB)** | PP-DocLayoutV3 (0.13 GB) | MangaOCR / PaddleOCR-VL GGUF (1.82 GB) | AOT / LaMa (0.20 GB) | ~4.5 GB |
| 12–16 GB | **Gemma 4 E4B Q4_K_XL (4.22 GB)** | RF-DETR Seg 2XL (0.16 GB) | PaddleOCR-VL GGUF / MangaOCR | LaMa | ~5.5 GB |

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
      build, artifact upload; pushing a tag matching `android-m0-*` builds the debug arm64
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
- [ ] Install and boot on a physical phone
- [ ] WebGPU probe on hardware

**Spike to run in this milestone:** log `navigator.gpu` availability and adapter
`UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` from the Android WebView. This single result decides
whether `koharu-canvas` works as-is or needs a fallback (see risks).

Emulator result: `navigator.gpu` is present but `navigator.gpu.requestAdapter()` returns
`null` — the emulator renders in software (SwiftShader), so
`UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` could not be checked. The emulator cannot decide the
canvas path; rerun the probe on a physical GPU.

---

### M1 — Platform services

**Goal:** the app can open real files, save keys, and survive long jobs.

- [ ] New `koharu-android` plugin crate (Rust + Android/Kotlin): SAF picker, create-document,
      share sheet, Keystore, foreground service, back button
- [ ] Picker abstraction in `koharu-app`; `rfd` stays the desktop implementation.
      Import: multi-select ZIP/CBZ/PDF/RAR/images. Export: PNG/PSD/CBZ.
      Handle `content://` URIs by copying into app cache before decode.
- [ ] Keystore-backed store in `koharu-secrets` (same get/set/delete API; BYOK keys persist)
- [ ] Foreground service + notification progress for `process` and model/font downloads;
      partial wakelock; stop on cancel
- [ ] Back button: close sheet/dialog → cancel gesture → background the app
- [ ] `RunEvent::Resumed`/`Suspended`, orientation changes, soft-keyboard insets
- [ ] `INTERNET` permission; first-run storage note
- [ ] Fonts: fetch into app-private storage, not an evictable cache path

**Exit criteria:** import a CBZ, open the project, export a PNG, save an API key, run a long
job with the screen off.

**Risk spike:** confirm `tauri-plugin-dialog` returns usable Android results before writing a
JNI picker.

---

### M2 — llama.cpp engine

**Goal:** offline translation with Gemma 4 E4B, measured.

- [ ] Publish `aarch64-linux-android` `libllama`/`libmtmd` from `koharu-rs/llama`
      (NDK r27c, API 31, `GGML_OPENMP=OFF`, `GGML_LLAMAFILE=OFF`, KleidiAI on,
      `-Wl,-z,max-page-size=16384` for Play's 16 KB page rule)
- [ ] `Bundled` package source in `koharu-runtime`; Android arms in `Llama::discover` and
      `library_names()`
- [ ] Android hardware probe: CPU device always; Vulkan behind a setting, off by default
      (Adreno Vulkan/OpenCL is frequently slower than CPU for small models and unstable)
- [ ] Mobile load profile: `gpu_layers = 0`, thread count from performance-core topology,
      mmap, capped `n_ctx`, q8 KV cache, lean `n_batch`/`n_ubatch`
- [ ] RAM class detection (`ActivityManager.getMemoryClass()` and `/proc/meminfo`) → profile
      selection; refuse loads that exceed budget with a readable error
- [ ] `koharu-bindgen`: Android arm in `already_loaded_library` (`rewrite.rs:146-151`)
- [ ] Gemma 4 E4B end-to-end translation; BYOK providers verified on device
- [ ] Benchmark harness + numbers per reference device (tok/s, page latency, peak RSS,
      thermal status); document in `packages/docs/hardware.mdx`

**Exit criteria:** a page translated fully on-device in airplane mode, with recorded numbers
and no OOM.

---

### M3 — libtorch, full pipeline

**Goal:** detection + OCR + inpainting on-device, unattended batch runs.

- [ ] Cross-compile torch 2.13 (`libtorch_cpu.so`) + `libkoharu-torch.so` for
      `aarch64-linux-android`; publish to `koharu-rs/torch`; bundle via jniLibs or an AAB
      feature module
- [ ] Android arms in `Torch::discover` / `library_names()`; map `Vulkan` → CPU until validated
- [ ] Mobile memory policy: one resident stage, evict between stages; extend CPU-lane OOM
      `recover()` (`pipeline/src/accelerator.rs:39-48`) to unload other models and retry once
- [ ] Detection model decision. RF-DETR Seg 2XL is the only pipeline-selectable detector and
      runs at 1152²; either run it at reduced resolution or wire PP-DocLayoutV3 (0.13 GB) /
      YOLO26s (0.046 GB) into the same output contract (text/onomatopoeia/bubble/panel + masks).
      Two-way spike, pick on quality vs latency.
- [ ] Selectable mobile defaults in `pipeline/src/config.rs`; AOT default inpainting, LaMa optional
- [ ] OCR defaults: MangaOCR for Japanese, PaddleOCR-VL GGUF for other scripts (reuses the
      llama runtime, no torch)
- [ ] Parity check: 5-page set, desktop vs Android → boxes, masks, OCR text, translations;
      record diffs

**Exit criteria:** a chapter processes unattended with no OOM kills and a parity report is
attached to the milestone.

---

### M4 — Touch UI

**Goal:** import → edit → typeset → export, one-handed.

- [ ] Mobile app bar + overflow menu; desktop shortcut labels (window chrome and updater are already hidden on mobile in M0)
- [ ] Canvas-first layout; Pages/Inspector/Agent/Activity become bottom sheets
      (`packages/ui` already ships `sheet.tsx`, `drawer.tsx`, `sidebar.tsx`);
      horizontal bottom toolbar; compact status bar
- [ ] Gestures in `CanvasWorkspace.tsx`: pinch-zoom, two-finger pan, long-press, touch slop,
      guard against a second pointer; keep single-finger draw
- [ ] Touch-sized targets (≥44 px hit area, visual size unchanged) in `SelectionControls.tsx`,
      `Inspector.tsx`, `ToolBar.tsx`; explicit multi-select mode instead of Shift/Ctrl;
      pointer-based page reorder; explicit rename
- [ ] Keyboard-aware text editing (canvas overlay or bottom sheet); `dvh`; viewport metadata
- [ ] Hide the Agent panel on Android v1 (commands stay compiled)
- [ ] Locale keys for every new string in all 11 locales
- [ ] Screenshots at 360×800, 412×915, 800×1280 (plus a long-label case)

**Exit criteria:** a page can be imported, edited, typeset, and exported on a phone without
a keyboard, mouse, or hover.

---

### M5 — Perf, thermal, device matrix

- [ ] Reference devices: 8 GB mid-range, 12 GB flagship, 6 GB floor
- [ ] Sustained 10-page run: thermal throttling, LMK behavior, battery drain
- [ ] Battery-saver profile (fewer threads, smaller quant, lower concurrency)
- [ ] Decide whether RF-DETR survives on mobile; publish the supported matrix

---

### M6 — Release

- [ ] CI Android job: JDK, SDK, NDK, `tauri android build`, AAB/APK artifacts, keystore
      signing, `versionCode` synced from release-please
- [ ] Play internal track, arm64 ABI split, native symbol upload (`.github/sentry.sh` has no
      Android branch)
- [ ] Docs: Android tab in `installation`, `hardware`, `quickstart` across en/ja/zh
- [ ] Upstream discussion issue + split PR series (required by `CONTRIBUTING.md`)

---

## Risk register

| Risk | Impact | Mitigation | Where |
|---|---|---|---|
| WebGPU missing in Android **WebView**, or `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` absent | Canvas unusable | Probe run in M0: the emulator (SwiftShader) reports `navigator.gpu` but `requestAdapter()` returns `null`, so hardware must confirm; if absent, add a native SurfaceView present path or a CPU-composited fallback before M4 | M0 |
| libtorch Android build, size, or operator coverage | M3 slips | Timebox a build spike first; fall back to an AAB feature module; selective-ops build only if size forces it | M3 |
| Gemma 4 E4B memory on 8 GB | OOM | RAM-class profiles, text-only translation, E2B below 8 GB | M2 |
| Sustained decode speed | Poor UX | Queue + foreground service, per-page progress, fast profile (Qwen 3.5 0.8B) | M2/M4 |
| SAF URI handling via plugin | Import/export broken | Spike `tauri-plugin-dialog` before writing a JNI picker | M1 |
| Play policy on bundled native runtimes | Distribution | Bundle `.so` only; never download executable code | M0/M6 |
| Detection model contract mismatch | Pipeline quality | Two-way spike with parity comparison | M3 |
| `native-tls`/OpenSSL in the NDK build | Build failure | Switch to rustls first | M0 |

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

- Detection: reduced-resolution RF-DETR vs. wiring PP-DocLayoutV3/YOLO26s (decided in M3).
- Whether to ship Vulkan at all on Android, or CPU-only for v1 (decided in M2 benchmarks).
- APK size budget if libtorch cannot be trimmed: AAB feature module vs. Play asset delivery.

---

## Session log

Append one entry per working session: date, milestone, what changed, what was verified,
and what the next session should pick up.

| Date | Milestone | Summary | Next |
|---|---|---|---|
| 2026-10-21 | Plan | Repository surveyed; blockers, model profile, and milestones defined. No code changes. | Install toolchain, create `android` branch, start M0 |
| 2026-10-21 | M0 | Installed toolchain and host deps. `koharu-app` now builds on `DynRuntime` (CEF/Wry chosen by the entry), plugins and dialogs split by target, Android paths injected, ML init lazy, secrets fail soft on Android, `koharu-bindgen` passes the NDK sysroot. `cargo ndk check` and `tauri android build --debug --target aarch64` succeed; APK is `rs.koharu.app`, `minSdk 31`, `arm64-v8a`; desktop `cargo check` still passes. | Install the APK on a device/emulator, verify the shell leaves `StartupView`, run the WebGPU probe, then exercise the CI workflow |
| 2026-10-21 | M0 fixes | `process` loads the pipeline before registering the stop token/job, so a failed model load no longer leaves a phantom running job or blocks later requests with "another process is already running"; `koharu_config::configure` rejects configuration after the manager was created and `path()` is documented with the override; the unused workspace `open` dependency was removed; the desktop Settings panel keeps a square top-right corner (`md:rounded-tr-none`); the mobile startup screen hides desktop window chrome and respects safe-area insets. Verified on the emulator (Android 15, `sdk_gphone64_x86_64`): fresh 1.6 GB debug APK installs and boots after `adb uninstall rs.koharu.app` (`INSTALL_FAILED_INSUFFICIENT_STORAGE` otherwise), StartView and Settings render, storage hint and desktop title bar gone on mobile, Settings uses the stacked mobile layout with safe-area padding, assets come from `http://tauri.localhost/` with no dev server. WebGPU probe: `navigator.gpu` present, `requestAdapter()` returns `null` (SwiftShader), `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` unchecked. No physical phone tested. Hosts without Android Studio must export `JAVA_HOME` (21 works) — the Tauri CLI only auto-detects `/opt/android-studio/jbr` or `java` on `PATH`. `cargo-ndk` 4.1.2 is the tested version, now pinned in `android.yml`; CI has not run. | Install and boot on a physical phone, rerun the WebGPU probe on hardware, get the first CI run green |
| 2026-10-21 | M0 review | Fixed `clippy::redundant_closure` in `koharu-bindgen`. Hid the Providers tab on mobile until the M1 Keystore store: a typed credential made "Back to editor" fail forever. Corrected the plan's stale Settings, startup, and external-link rows, the M4 Settings/safe-area overlap, the `adb reverse` note, and the desktop-error note (command errors are already toasted by `call()` in `packages/koharu/lib/backend.ts`). Added tag-triggered preview releases (`android-m0-*`, `CARGO_PROFILE_DEV_DEBUG=0`). CI run not verified yet. | Rerun CI, install and boot on a physical phone, rerun the WebGPU probe on hardware |

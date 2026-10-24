# Android runtime build (the scripts)

Authoritative sources for the two native runtimes the Android app bundles:
`libllama.so` + `libmtmd.so` (llama.cpp) and libtorch + the `koharu-torch` shim.
Neither upstream publishes an Android asset, so both are cross-compiled on the
dev host. `Android_build_plan.md` describes the result; this directory holds the
recipe.

The build root is `$ANDROID_RUNTIME_BUILD_ROOT`, default
`$XDG_CACHE_HOME/koharu-android-runtimes` (or `~/.cache/koharu-android-runtimes`).
It holds the checkouts, the CMake build trees, and the staged `out/`. Nothing
here is committed: keep binaries, install trees, logs, and source checkouts out
of the repository.

## Contents

| File | Purpose |
| --- | --- |
| `build.sh` | Clone llama.cpp `b11642`, fetch the `koharu-rs/llama` wrapper, apply `llama-android-cpu-backend.patch`, cross-compile `llama` + `mtmd`, stage `out/llama/`. |
| `build-acl.sh` | Cross-compile Arm Compute Library `v52.8.0` as a **static** library for `aarch64-linux-android`, stage `acl-install/`. Feeds oneDNN's `DNNL_AARCH64_USE_ACL` backend. |
| `configure.sh` | Configure a patched PyTorch `v2.13.0` checkout for `aarch64-linux-android` API 31. |
| `verify.sh` | `readelf`/`nm` checks for an `out/llama` directory: `DT_NEEDED`, 16 KB `LOAD` alignment, exported symbol counts, and a host-path/OpenMP scan. |
| `llama-android-cpu-backend.patch` | llama wrapper patch (see below). |
| `koharu-pytorch-android.patch` | PyTorch `v2.13.0` source patch (see below). |
| `koharu-mimalloc-android.patch` | mimalloc submodule patch (see below). |

## oneDNN ACL variant

`configure.sh` builds oneDNN (`third_party/ideep/mkl-dnn`) with
`-DUSE_MKLDNN=ON -DUSE_MKLDNN_ACL=ON -DDNNL_CPU_RUNTIME=SEQ`, backed by the
static ACL from `build-acl.sh`. This targets the same problem as the reverted
XNNPACK experiment, without its limitation: eager XNNPACK convolution requires
`input_channels == groups` (the depthwise gate), so ordinary *dilated*
convolutions stay on `slow_conv_dilated2d_cpu` (im2col). oneDNN's ACL backend
dispatches them to ACL's convolution instead.

Verified on the resulting `libtorch_cpu.so`:

- `-DDNNL_AARCH64_USE_ACL` reaches the oneDNN TUs; `acl_convolution_utils.cpp`,
  `acl_gemm_convolution.cpp`, etc. compile, and `acl_gemm_convolution_fwd_t`,
  plus ~4000 `arm_compute::` symbols, are statically linked in.
- `DT_NEEDED` stays `libc10.so, liblog.so, libdl.so, libm.so, libc++_shared.so,
  libc.so` — no `libomp`, `libgomp`, `libarm_compute`, or `libdnnl`. ACL is a
  static archive, so the five-library bundle contract in
  `crates/koharu-runtime/src/runtime/packages/torch.rs` is unchanged.
- All `LOAD` segments at `0x4000` (16 KB page) alignment.
- `libc10.so` has zero `TLS_TPREL64` relocations (mimalloc Android patch holds),
  so `dlopen` on bionic is safe.
- Unstripped `libtorch_cpu.so` is 754 MB (debug info); `llvm-strip --strip-debug`
  gives 192 MB, still 16 KB-aligned.


## Pinned revisions

- llama.cpp `ggml-org/llama.cpp` tag **`b11642`** — its `llama.h`/`ggml.h` are
  byte-identical to `crates/koharu-llama-sys/include`, so the pin is not
  optional.
- Wrapper: `koharu-rs/llama` `main`, `cmake/CMakeLists.txt` + `cmake/ggml-static.cmake`.
- PyTorch **`v2.13.0`** (`cf30153c4c131c8164ee7798e5022d810682e2cb`).
- oneDNN **`v3.12`** — vendored by PyTorch at `third_party/ideep/mkl-dnn`
  (`80afa71`); no separate fetch.
- Arm Compute Library **`v52.8.0`** (`cffb5d67ad74e6ed924c708d4981ba15b644bc05`)
  — built by `build-acl.sh` only when the oneDNN ACL backend is enabled. oneDNN
  requires ACL >= 52.4 (`mkl-dnn/cmake/ACL.cmake`).
- mimalloc submodule `02a2f5df9d7d46d30263b83832eebeeab62dc5fe`
  (`https://github.com/microsoft/mimalloc.git`).
- Shim: `koharu-rs/torch` (`shim/shim/`), built against the libtorch install tree.

## Patches

### `llama-android-cpu-backend.patch`

Applies to the `koharu-rs/llama` wrapper checkout with `patch -p1` from the
wrapper root (`build.sh` does this). The wrapper only accepts
CUDA/HIP/VULKAN/METAL; this adds a `CPU` backend and dedupes `ggml-cpu` from the
`WHOLE_ARCHIVE` target list, which would otherwise list the same static library
twice.

### `koharu-pytorch-android.patch`

Applies to the PyTorch root with `git apply` (or `patch -p1`). Seven edits:

1. `CMakeLists.txt` — gate `INTERN_BUILD_MOBILE` with `AND NOT KOHARU_FULL_LIBTORCH`.
   Android otherwise forces a mobile build (`NO_API` + autograd off), which drops
   the C++ frontend and autograd engine the shim links against.
2. `cmake/Dependencies.cmake` — add `ANDROID` to the pthreadpool gate.
   `C10_MOBILE` is still defined, so `ATen/ParallelNative.cpp` needs
   `caffe2::pthreadpool`.
3. `cmake/Dependencies.cmake` — link the full `onnx` library on Android when
   `KOHARU_FULL_LIBTORCH`. The non-mobile JIT serialization sources call ONNX's
   checker and shape inference, not just the proto defs.
4. `aten/src/ATen/DynamicLibrary.cpp` — compile its implementation even under
   `C10_MOBILE`, which the JIT CPU fuser needs.
5. `caffe2/CMakeLists.txt` — compile `aten/src/ATen/core/dynamic_type.cpp` with
   `-fvisibility=default`. The JIT IR header instantiates
   `c10::DynamicType::fallback()` from a template the shim uses, and the global
   `-fvisibility=hidden` keeps it out of `libtorch_cpu.so`'s dynamic symbol
   table (`cannot locate symbol _ZNK3c1011DynamicType8fallbackEv`).
6. `aten/src/ATen/native/mkldnn/IDeepRegistration.cpp` — under `C10_MOBILE`,
   back ideep's engine allocator with `c10::alloc_cpu`/`c10::free_cpu` instead
   of `GetAllocator(CPU)->raw_allocate`. The mobile `DefaultMobileCPUAllocator`
   returns `data + PreGuard` and its context is `data`, so ideep's
   `dptr.get() == dptr.get_context()` assertion aborts the first oneDNN
   `conv2d`. Desktop keeps the `raw_allocate` path.
7. `c10/core/impl/alloc_cpu.cpp` — move the `USE_MIMALLOC` branch of `alloc_cpu`
   ahead of the `__ANDROID__` `memalign` branch. Upstream tests `__ANDROID__`
   first, so `-DUSE_MIMALLOC=ON` is a no-op on Android and bionic's `memalign`
   would be paired with `mi_free` in `free_cpu`. `free_cpu` already tests
   `USE_MIMALLOC` first, so it needs no edit.

The four `third_party/{cutlass,flash-attention,kineto,mslk}` `Subproject
commit …-dirty` hunks are checkout dirt, not build requirements, and are not in
this patch.

### `koharu-mimalloc-android.patch`

Applies to the **mimalloc submodule**
(`third_party/mimalloc/`, a git implementation of its own; `git -C
third_party/mimalloc apply` — the superproject's `git apply` cannot reach a
gitlink path). Kept separate from the PyTorch patch for that reason. Drops
`-ftls-model=initial-exec` on Android: mimalloc is linked into `libc10.so` and
bionic refuses an initial-exec TLS relocation in a `dlopen`ed library
(`TLS symbol "(null)" … using IE access model`), which is how the app loads its
bundled runtimes. Check with
`readelf -rW libc10.so | grep -c TLS_TPREL64` — it must be 0.

## Flags that matter

llama.cpp (`build.sh`):

- `-DLLAMA_BACKEND=CPU`, `-DGGML_CPU_KLEIDIAI=ON`. The wrapper forces
  `GGML_OPENMP=OFF` and `GGML_LLAMAFILE=OFF`. The headers fold the ggml static
  targets into `libllama.so`, so `DT_NEEDED` stays libm/libdl/libc (no `liblog`).
- NDK toolchain, `-DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-31`,
  `-Wl,-z,max-page-size=16384` (16 KB pages).

libtorch (`configure.sh`):

- `KOHARU_FULL_LIBTORCH=ON` (exported as an env var too) — the opt-out of the
  CMake mobile path the patch gates on.
- `-DUSE_OPENMP=OFF` — the runtime props list no OpenMP runtime, so libtorch
  must not need one.
- `-DUSE_MIMALLOC=ON -DMI_NO_OPT_ARCH=ON` — `libc10.so` allocates through
  mimalloc instead of bionic `memalign` (see patch 7). mimalloc is a static
  archive, so `DT_NEEDED` does not change. `MI_NO_OPT_ARCH` keeps the plain
  arm64-v8a baseline: mimalloc otherwise compiles with `-march=armv8.1-a`.
- `-DUSE_KLEIDIAI=OFF` — defaults ON for `CPU_AARCH64`; not part of the recipe.
- `-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON` — NDK r27c does not add
  `-Wl,-z,max-page-size=16384` by default; this turns it on.
- `-DCAFFE2_CUSTOM_PROTOC_EXECUTABLE=<src>/build_host_protoc/bin/protoc` — a
  cross build cannot run its own `protoc`.
- `-DBLAS=Eigen -DUSE_MKLDNN=ON -DUSE_MKLDNN_ACL=ON -DDNNL_CPU_RUNTIME=SEQ
  -DUSE_FBGEMM=OFF -DUSE_XNNPACK=OFF -DUSE_PYTORCH_QNNPACK=OFF`, Eigen under
  `third_party/eigen`, with `ACL_ROOT_DIR` pointing at `build-acl.sh`'s install
  tree. oneDNN's ACL backend (via `cmake/public/ComputeLibrary.cmake`) dispatches
  ordinary dilated convolutions to ACL instead of `slow_conv_dilated2d_cpu`
  (im2col). XNNPACK cannot serve these: eager XNNPACK conv requires
  `input_channels == groups` (depthwise gate), so non-depthwise dilated convs
  never reach it. `DNNL_CPU_RUNTIME=SEQ` is required because the bundle ships no
  OpenMP runtime and `USE_OPENMP=OFF` is set for the same reason.

The shim (`koharu-rs/torch`, built separately against the install tree):

- `-DTorch_DIR=<install>/share/cmake/Torch` with
  `CMAKE_FIND_ROOT_PATH_MODE_* = BOTH` — the NDK toolchain restricts config-mode
  package search to the sysroot.
- `-DANDROID_STL=c++_shared` — without it the NDK statically links libc++ into
  the shim while libtorch uses `libc++_shared.so`. The two C++ runtimes have
  different exception RTTI, so a C++ exception from libtorch escapes the shim's
  `catch (const std::exception&)` and aborts Rust with
  `Rust cannot catch foreign exceptions`.
- `-DCMAKE_SHARED_LINKER_FLAGS="-Wl,-z,undefs"` — the NDK links shared objects
  with `-z defs` while desktop allows undefined symbols; `-z undefs` matches
  desktop's lazy resolution.
- Yields `libkoharu-torch.so` (2734 exported `at_*`/`atg_*` symbols).

## Apply and build

```sh
root=${ANDROID_RUNTIME_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/koharu-android-runtimes}
mkdir -p "$root" && cd "$root"

# llama.cpp
scripts_dir=/path/to/repo/scripts/android-runtimes   # this directory
bash "$scripts_dir/build.sh"

# libtorch
git clone --depth 1 --branch v2.13.0 --recurse-submodules \
  https://github.com/pytorch/pytorch.git pytorch
git -C pytorch apply "$scripts_dir/koharu-pytorch-android.patch"
git -C pytorch/third_party/mimalloc apply "$scripts_dir/koharu-mimalloc-android.patch"

# To reuse an already-patched checkout outside $root (for example to add a
# second build configuration without duplicating the ~2.7 GB source tree), skip
# the clone above and set ANDROID_TORCH_SOURCE to it. `build/` and `install/`
# still land under $root, so configurations do not overwrite each other:
#   root=$HOME/build-android-onednn
#   ANDROID_TORCH_SOURCE=$HOME/build-android/pytorch bash "$scripts_dir/configure.sh"

# `third_party/eigen` is not in PyTorch's `.gitmodules`, so `--recurse-submodules`
# does not fetch it. `-DBLAS=Eigen` needs the 5.0.1 tree (`bc3b39870ecb`).
git clone --depth 1 --branch 5.0.1 \
  https://gitlab.com/libeigen/eigen.git pytorch/third_party/eigen

# A cross build cannot run the protoc it builds, so build one on the host first.
(cd pytorch && ./scripts/build_host_protoc.sh)

# Arm Compute Library (static) — only needed when using the oneDNN ACL backend.
bash "$scripts_dir/build-acl.sh"

bash "$scripts_dir/configure.sh"

# The non-mobile JIT serialization sources include `onnx/onnx_pb.h`; CMake
# generates the protobuf headers in the build tree, but the hand-written ONNX
# headers (checker.h, proto_utils.h, ...) have to be copied in beside them.
mkdir -p "$root/build/third_party/onnx/onnx"
cp pytorch/third_party/onnx/onnx/*.h "$root/build/third_party/onnx/onnx/"

cmake --build "$root/build" --target install -j "$(nproc)"
```

### Disk

The libtorch recipe needs `build/` + `install/` under `$root` (about 8-9 GB
combined at the `libtorch_cpu.so` link, which is the peak). It does **not** need
its own source copy: point `ANDROID_TORCH_SOURCE` at an existing patched checkout
and reuse the host-native sleef tree. The oneDNN ACL variant can be built this
way into a separate `$HOME/build-android-onednn`, sharing
`$HOME/build-android/pytorch`, with 18 GB free on `/`. Reuse the
host-native sleef by symlinking it into `$root` (`ln -s <old>/sleef-native
$root/sleef-native`) rather than rebuilding it; `NATIVE_BUILD_DIR` must be a
host-x86_64 tree, not an Android one.

Do not commit the checkouts, the `build/` or `install/` trees, the staged
`out/`, or any logs. Only the scripts and patches above belong in git.

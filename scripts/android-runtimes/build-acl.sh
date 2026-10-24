#!/usr/bin/env bash
# Build Arm Compute Library (ACL) v52.8.0 as a STATIC library for
# aarch64-linux-android (API 31), for oneDNN's DNNL_AARCH64_USE_ACL backend.
#
# ACL is not vendored by PyTorch and has no Android release asset, so oneDNN's
# FindACL (ACL_ROOT_DIR / arm_compute + arm_compute_graph) needs a local static
# build. Static (standalone=1) keeps the result out of libtorch_cpu.so's
# DT_NEEDED list, which the Android bundle contract (runtime/packages/torch.rs)
# caps at the five torch libraries.
#
# Prereqs: scons (pip), NDK r27c at $ANDROID_NDK_HOME.
# Outputs into $ROOT/acl-install ({include,lib}); libs are libarm_compute.a +
# libarm_compute_graph.a.
set -euo pipefail

ROOT="${ANDROID_RUNTIME_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/koharu-android-runtimes}"
ACL_SRC="$ROOT/acl"
ACL_BUILD="$ROOT/acl-build"
ACL_INSTALL="$ROOT/acl-install"
NDK="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/27.2.12479018}"
TRIPLE=aarch64-linux-android31
JOBS="${ACL_JOBS:-8}"

if [ ! -d "$ACL_SRC/.git" ]; then
  git clone --depth 1 --branch v52.8.0 \
    https://github.com/ARM-software/ComputeLibrary.git "$ACL_SRC"
fi

mkdir -p "$ACL_BUILD" "$ACL_INSTALL"

# ACL sets AR/RANLIB to `toolchain_prefix + ar/ranlib`, i.e. the bare
# `aarch64-linux-android-ar`. The NDK r27 bin directory ships only `llvm-ar` /
# `llvm-ranlib`, so provide the prefixed names via a private shim dir on PATH.
NDK_BIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
SHIM="$ACL_BUILD/.toolchain-bin"
mkdir -p "$SHIM"
for pair in "ar:llvm-ar" "ranlib:llvm-ranlib" "nm:llvm-nm" "strip:llvm-strip" "objcopy:llvm-objcopy" "objdump:llvm-objdump"; do
  name="${pair%%:*}"; tool="${pair##*:}"
  [ -e "$SHIM/aarch64-linux-android-$name" ] || \
    ln -sf "$NDK_BIN/$tool" "$SHIM/aarch64-linux-android-$name"
done
export PATH="$SHIM:$NDK_BIN:$PATH"

# opencl=0: CPU/Neon only. openmp=0: no libomp dependency (the bundle carries no
# OpenMP runtime). cppthreads=1: std::thread scheduler. standalone=1: static
# archives linked -static-libstdc++/-static-libgcc, which is what FindACL wants.
scons -C "$ACL_SRC" -j "$JOBS" \
  os=android \
  arch=arm64-v8a \
  build_dir="$ACL_BUILD" \
  install_dir="$ACL_INSTALL" \
  compiler_prefix="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/$TRIPLE-" \
  toolchain_prefix=auto \
  neon=1 opencl=0 openmp=0 cppthreads=1 standalone=1 \
  data_type_support=all build=cross_compile debug=0 \
  examples=0 gemm_tuner=0 Werror=0

# oneDNN's FindACL looks for `libarm_compute` / `libarm_compute_graph`; with
# standalone=1 ACL names the static archives with a `-static` suffix. Provide
# the unsuffixed names FindACL expects.
for name in arm_compute arm_compute_graph; do
  ln -sf "lib${name}-static.a" "$ACL_INSTALL/lib/lib${name}.a"
done

# oneDNN's ACL.cmake reads the version from `arm_compute_version.embed` under
# ACL_INCLUDE_DIR; ACL installs headers but not this generated file, so place it
# where FindACL globs for it.
cp "$ACL_BUILD/src/core/arm_compute_version.embed" "$ACL_INSTALL/include/"

echo "ACL static libs:"
ls -la "$ACL_INSTALL/lib/"*.a

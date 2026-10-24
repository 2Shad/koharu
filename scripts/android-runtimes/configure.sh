#!/bin/bash
# Configure libtorch v2.13.0 for aarch64-linux-android (API 31).
#
# Deviations from the desktop recipe (all documented in the final report):
#   * CMakeLists.txt gated: INTERN_BUILD_MOBILE is disabled when the env var
#     KOHARU_FULL_LIBTORCH is set.  Android otherwise forces a "mobile" build
#     (NO_API ON + autograd disabled), which removes the C++ frontend and the
#     autograd engine the koharu-torch shim links against.
#   * -DCAFFE2_CUSTOM_PROTOC_EXECUTABLE: cross compile, so the target build
#     cannot produce a runnable protoc; scripts/build_host_protoc.sh supplies one.
#   * -DUSE_KLEIDIAI=OFF: defaults ON for CPU_AARCH64; not part of the recipe.
#   * -DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON: NDK r27c does not add
#     -Wl,-z,max-page-size=16384 by default; this turns it on.
#   * -DUSE_ONNX=OFF is NOT used: the desktop recipe builds ONNX and the
#     non-mobile jit serialization sources need onnx.pb.h.
#   * -DUSE_MKLDNN=ON -DUSE_MKLDNN_ACL=ON + ACL_ROOT_DIR: oneDNN (vendored at
#     third_party/ideep/mkl-dnn, v3.12) with its Arm Compute Library backend.
#     This is the alternative to XNNPACK for dilated convolutions: eager XNNPACK
#     conv requires input_channels == groups (depthwise gate), so ordinary
#     dilated convs fall back to slow_conv_dilated2d_cpu (im2col). oneDNN's ACL
#     backend dispatches them to ACL's direct/Winograd convolution. ACL is built
#     statically by scripts/android-runtimes/build-acl.sh (see README).
#   * -DDNNL_CPU_RUNTIME=SEQ: oneDNN defaults to the OMP runtime, but the
#     Android bundle carries no OpenMP runtime and this build sets
#     -DUSE_OPENMP=OFF. Sequential oneDNN relies on PyTorch's own intra-op
#     parallelism (at::parallel_for) instead.
#   * -DCMAKE_FIND_ROOT_PATH_MODE_{INCLUDE,LIBRARY}=BOTH: the NDK default
#     toolchain sets these to ONLY, so FindMKLDNN/FindACL's FIND_PATH/
#     FIND_LIBRARY calls with absolute in-source paths (third_party/ideep,
#     ACL_ROOT_DIR) are rejected and oneDNN silently drops out. Same class of
#     restriction the shim works around with CMAKE_FIND_ROOT_PATH_MODE_*.
#   * -DACL_INCLUDE_DIR/-DACL_LIBRARY/-DACL_GRAPH_LIBRARY: the upstream
#     FindACL.cmake does `find_path(ACL_INCLUDE_DIR NAMES arm_compute/graph.h
#     PATHS ENV ACL_ROOT_DIR)` without `PATH_SUFFIXES include`, so it looks for
#     <ACL_ROOT_DIR>/arm_compute/graph.h while the ACL install tree puts headers
#     under <ACL_ROOT_DIR>/include/. Passing the cache vars directly sidesteps
#     the wrong search path (the ACL_LIBRARY search does use PATH_SUFFIXES lib,
#     which is why only the include dir fails).
#   * -DUSE_MIMALLOC=ON -DMI_NO_OPT_ARCH=ON: c10's alloc_cpu uses
#     mi_malloc_aligned instead of bionic memalign (the patch moves that branch
#     ahead of __ANDROID__). mimalloc links statically into libc10, so the
#     DT_NEEDED set is unchanged. MI_NO_OPT_ARCH keeps the arm64-v8a baseline;
#     mimalloc otherwise compiles with -march=armv8.1-a (LSE atomics).
#
# Expects $ROOT/pytorch to be a PyTorch v2.13.0 checkout with both patches from
# this directory applied (see README.md).  Set ANDROID_RUNTIME_BUILD_ROOT to
# reuse an existing tree; it defaults to the same cache directory build.sh uses.
# Set ANDROID_TORCH_SOURCE to point at a checkout outside $ROOT (e.g. a shared
# patched tree) while still writing build/ and install/ under $ROOT.
set -euo pipefail

NDK="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/27.2.12479018}"
ROOT="${ANDROID_RUNTIME_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/koharu-android-runtimes}"
SRC="${ANDROID_TORCH_SOURCE:-$ROOT/pytorch}"
BUILD="$ROOT/build"
INSTALL="$ROOT/install"

# oneDNN's FindACL looks for arm_compute/graph.h and libarm_compute.a under
# ACL_ROOT_DIR; build-acl.sh installs there.
export ACL_ROOT_DIR="${ACL_ROOT_DIR:-$ROOT/acl-install}"

export KOHARU_FULL_LIBTORCH=1

cmake -S "$SRC" -B "$BUILD" -G Ninja \
  -DKOHARU_FULL_LIBTORCH=ON \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$INSTALL" \
  -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
  -DANDROID_NDK="$NDK" \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM=android-31 \
  -DANDROID_STL=c++_shared \
  -DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON \
  -DCAFFE2_CUSTOM_PROTOC_EXECUTABLE="$SRC/build_host_protoc/bin/protoc" \
  -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=BOTH \
  -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=BOTH \
  -DACL_INCLUDE_DIR="$ACL_ROOT_DIR/include" \
  -DACL_LIBRARY="$ACL_ROOT_DIR/lib/libarm_compute.a" \
  -DACL_GRAPH_LIBRARY="$ACL_ROOT_DIR/lib/libarm_compute_graph.a" \
  -DNATIVE_BUILD_DIR="$ROOT/sleef-native" \
  -DPYTHON_EXECUTABLE=/usr/bin/python3 \
  -DPython_EXECUTABLE=/usr/bin/python3 \
  -DPython3_EXECUTABLE=/usr/bin/python3 \
  -DBLAS=Eigen -DBUILD_BINARY=OFF -DBUILD_FUNCTORCH=OFF -DBUILD_JNI=OFF \
  -DBUILD_MOBILE_AUTOGRAD=OFF -DBUILD_MOBILE_BENCHMARK=OFF -DBUILD_MOBILE_TEST=OFF \
  -DBUILD_PYTHON=OFF -DBUILD_SHARED_LIBS=ON -DBUILD_STATIC_RUNTIME_BENCHMARK=OFF \
  -DBUILD_TEST=OFF -DINSTALL_TEST=OFF \
  -DUSE_CUDA=OFF -DUSE_ROCM=OFF -DUSE_MPS=OFF -DUSE_NVRTC=OFF -DUSE_CUDNN=OFF \
  -DUSE_COREML_DELEGATE=OFF -DUSE_CUPTI_SO=OFF -DUSE_CUFILE=OFF -DUSE_CUSPARSELT=OFF \
  -DUSE_CUDSS=OFF -DUSE_DISTRIBUTED=OFF -DUSE_FLASH_ATTENTION=OFF -DUSE_GLOO=OFF \
  -DUSE_ITT=OFF -DUSE_KINETO=OFF -DUSE_MAGMA=OFF -DUSE_MEM_EFF_ATTENTION=OFF \
  -DUSE_MPI=OFF -DUSE_MSLK=OFF -DUSE_NCCL=OFF -DUSE_NNAPI=OFF -DUSE_NNPACK=OFF \
  -DUSE_NUMA=OFF -DUSE_NUMPY=OFF -DUSE_NVSHMEM=OFF -DUSE_OBSERVERS=OFF -DUSE_OPENCL=OFF \
  -DUSE_PROF=OFF -DUSE_PYTORCH_METAL=OFF -DUSE_PYTORCH_METAL_EXPORT=OFF \
  -DUSE_RELATIVE_PATHS=ON -DUSE_ROCM_CK_GEMM=OFF -DUSE_ROCM_CK_SDPA=OFF \
  -DUSE_ROCM_KERNEL_ASSERT=OFF -DUSE_TENSORPIPE=OFF -DUSE_VALGRIND=OFF -DUSE_VULKAN=OFF \
  -DUSE_XPU=OFF \
  -DBLAS=Eigen -DUSE_FBGEMM=OFF -DUSE_MKLDNN=ON -DUSE_MKLDNN_ACL=ON \
  -DDNNL_CPU_RUNTIME=SEQ \
  -DUSE_PYTORCH_QNNPACK=OFF -DUSE_XNNPACK=OFF \
  -DUSE_OPENMP=OFF \
  -DUSE_MIMALLOC=ON -DMI_NO_OPT_ARCH=ON \
  -DUSE_KLEIDIAI=OFF

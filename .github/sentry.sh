#!/usr/bin/env bash
set -euo pipefail

# Upload symbols before stripping; Tauri bundles and signs afterward.
# Desktop callers pass nothing and keep using RUNNER_OS; the Android release job
# passes "android" to upload the built jniLibs instead of a host binary.
target="${1:-${RUNNER_OS:-}}"

binary=target/release/koharu
case "$target" in
  macOS)
    binary=target/aarch64-apple-darwin/release/koharu
    dsymutil "$binary"
    sentry-cli debug-files upload --include-sources --wait "$binary.dSYM"
    strip -S -x "$binary"
    ;;
  Windows)
    sentry-cli debug-files upload --include-sources --wait "$binary.exe" "$binary.pdb"
    # Debug information is separate in the PDB, which Tauri does not bundle.
    ;;
  Linux)
    # Upload the full ELF so Sentry also has the unwind information.
    sentry-cli debug-files upload --include-sources --wait "$binary"
    llvm-strip --strip-debug "$binary"
    ;;
  android)
    # The cdylib and the bundled runtimes live side by side in jniLibs. Upload the
    # unstripped ELFs before Gradle signs the bundle; the signed AAB/APK only carries
    # the stripped copies produced by the AGP strip task.
    jni_libs="crates/koharu/gen/android/app/src/main/jniLibs/arm64-v8a"
    shopt -s nullglob
    libs=(
      "$jni_libs"/libkoharu*.so
      "$jni_libs"/libllama*.so
      "$jni_libs"/libmtmd.so
      "$jni_libs"/libtorch*.so
    )
    ((${#libs[@]})) || {
      echo "no Android native libraries found under $jni_libs" >&2
      exit 1
    }
    sentry-cli debug-files upload --include-sources --wait "${libs[@]}"
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 1
    ;;
esac

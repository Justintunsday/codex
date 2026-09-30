#!/usr/bin/env bash
set -euo pipefail
task_root="$(cd "$(dirname "$0")/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
export IPHONEOS_DEPLOYMENT_TARGET=16.0
export CARGO_TARGET_DIR="$task_root/ios/Build/Cargo"
case "${PLATFORM_NAME:-iphoneos}" in
  iphoneos) task_target=aarch64-apple-ios; task_sdk=iphoneos ;;
  iphonesimulator) task_target=aarch64-apple-ios-sim; task_sdk=iphonesimulator ;;
  *) printf 'Unsupported Apple platform: %s\n' "$PLATFORM_NAME" >&2; exit 1 ;;
esac
export SDKROOT="$(xcrun --sdk "$task_sdk" --show-sdk-path)"
rustup target add "$task_target" --toolchain 1.95.0
cd "$task_root/codex-rs"
task_profile=release
if [ "${CONFIGURATION:-Release}" = Debug ]; then
  task_profile=dev-small
fi
cargo +1.95.0 build --locked --profile "$task_profile" -p codex-ios-ffi --target "$task_target"
mkdir -p "$task_root/ios/Build/Rust/${PLATFORM_NAME:-iphoneos}"
cp "$CARGO_TARGET_DIR/$task_target/$task_profile/libcodex_ios_ffi.a" "$task_root/ios/Build/Rust/${PLATFORM_NAME:-iphoneos}/"

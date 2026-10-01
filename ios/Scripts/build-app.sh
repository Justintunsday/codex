#!/usr/bin/env bash
set -euo pipefail
task_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$task_root/ios"
command -v xcodegen >/dev/null || { printf 'Install xcodegen with brew install xcodegen\n' >&2; exit 1; }
xcodegen generate
xcodebuild -project Codex.xcodeproj -scheme Codex -configuration Release \
  -skipPackagePluginValidation \
  -destination 'generic/platform=iOS' -archivePath Build/Codex.xcarchive \
  CODE_SIGNING_ALLOWED=NO archive
ditto -c -k --keepParent Build/Codex.xcarchive/Products/Applications/Codex.app Build/Codex-unsigned.app.zip
mkdir -p Build/Payload
ditto Build/Codex.xcarchive/Products/Applications/Codex.app Build/Payload/Codex.app
(cd Build && zip -qr Codex-unsigned.ipa Payload)
printf 'Unsigned app built. Installation requires signing; see ios/README.md.\n'

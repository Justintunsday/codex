#!/usr/bin/env bash
set -euo pipefail
task_root="$(cd "$(dirname "$0")/../.." && pwd)"
: "${IOS_CERTIFICATE_BASE64:?Missing signing certificate}"
: "${IOS_CERTIFICATE_PASSWORD:?Missing certificate password}"
: "${IOS_PROFILE_BASE64:?Missing provisioning profile}"
: "${IOS_TEAM_ID:?Missing team ID}"
task_temp="$(mktemp -d)"
task_keychain="$task_temp/signing.keychain-db"
task_keychain_password="$(openssl rand -hex 24)"
task_profile_destination=""
cleanup() {
  security delete-keychain "$task_keychain" >/dev/null 2>&1 || true
  if [ -n "$task_profile_destination" ]; then rm -f "$task_profile_destination"; fi
  rm -rf "$task_temp"
}
trap cleanup EXIT
printf '%s' "$IOS_CERTIFICATE_BASE64" | base64 --decode > "$task_temp/certificate.p12"
printf '%s' "$IOS_PROFILE_BASE64" | base64 --decode > "$task_temp/profile.mobileprovision"
security create-keychain -p "$task_keychain_password" "$task_keychain"
security set-keychain-settings -lut 3600 "$task_keychain"
security unlock-keychain -p "$task_keychain_password" "$task_keychain"
security import "$task_temp/certificate.p12" -P "$IOS_CERTIFICATE_PASSWORD" -k "$task_keychain" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$task_keychain_password" "$task_keychain" >/dev/null
security list-keychains -d user -s "$task_keychain"
task_identity="$(security find-identity -v -p codesigning "$task_keychain" | awk '/\) / {print $2; exit}')"
if [ -z "$task_identity" ]; then printf 'No valid signing identity in certificate.\n' >&2; exit 1; fi
security cms -D -i "$task_temp/profile.mobileprovision" > "$task_temp/profile.plist"
task_profile_uuid="$(/usr/libexec/PlistBuddy -c 'Print UUID' "$task_temp/profile.plist")"
task_profile_name="$(/usr/libexec/PlistBuddy -c 'Print Name' "$task_temp/profile.plist")"
task_profile_team="$(/usr/libexec/PlistBuddy -c 'Print TeamIdentifier:0' "$task_temp/profile.plist")"
task_app_id="$(/usr/libexec/PlistBuddy -c 'Print Entitlements:application-identifier' "$task_temp/profile.plist")"
if [ "$task_profile_team" != "$IOS_TEAM_ID" ] || [ "$task_app_id" != "$IOS_TEAM_ID.org.codex.native-ios" ]; then
  printf 'Profile must match the team and org.codex.native-ios bundle identifier.\n' >&2; exit 1
fi
mkdir -p "$HOME/Library/MobileDevice/Provisioning Profiles"
task_profile_destination="$HOME/Library/MobileDevice/Provisioning Profiles/$task_profile_uuid.mobileprovision"
cp "$task_temp/profile.mobileprovision" "$task_profile_destination"
export TASK_PROFILE_NAME="$task_profile_name"
export TASK_EXPORT_METHOD="${IOS_EXPORT_METHOD:-development}"
python3 - "$task_temp/ExportOptions.plist" <<'PY'
import os, plistlib, sys
with open(sys.argv[1], 'wb') as output:
    plistlib.dump({'method': os.environ['TASK_EXPORT_METHOD'], 'teamID': os.environ['IOS_TEAM_ID'],
                  'signingStyle': 'manual', 'provisioningProfiles': {'org.codex.native-ios': os.environ['TASK_PROFILE_NAME']}}, output)
PY
cd "$task_root/ios"
xcodebuild -project Codex.xcodeproj -scheme Codex -configuration Release \
  -destination 'generic/platform=iOS' -archivePath Build/Codex-signed.xcarchive \
  DEVELOPMENT_TEAM="$IOS_TEAM_ID" PROVISIONING_PROFILE_SPECIFIER="$task_profile_name" \
  CODE_SIGN_IDENTITY="$task_identity" \
  OTHER_CODE_SIGN_FLAGS="--keychain $task_keychain" archive
xcodebuild -exportArchive -archivePath Build/Codex-signed.xcarchive \
  -exportPath Build/Signed -exportOptionsPlist "$task_temp/ExportOptions.plist"

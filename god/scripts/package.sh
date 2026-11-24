#!/bin/bash
# MIT License - Copyright (c) fintonlabs.com
# Build God.app, sign it with the Developer ID, notarise and staple it, and prove another Mac would open it.
# Leaves dist/mac-arm64/God.app and dist/God-<version>-mac.zip. It publishes nothing.
#
# Without the Developer ID certificate the app is ad-hoc signed (this Mac only). Without the notarytool keychain
# profile (NOTARY_KEYCHAIN_PROFILE, default "notarytool") it is signed but not notarised. With both, a refusal
# from Apple stops the script.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(node -p "require('./package.json').version")
profile="${NOTARY_KEYCHAIN_PROFILE:-notarytool}"
app="dist/mac-arm64/God.app"
zip="dist/God-$version-mac.zip"

npm test
# electron-builder signs with the Developer ID it finds in the keychain, hardened runtime on.
npx electron-builder --mac dir --publish never

signed=$(codesign -dvv "$app" 2>&1 || true)
if [[ "$signed" != *"Authority=Developer ID Application"* ]]; then
  echo "built, ad-hoc signed only: no Developer ID certificate on this machine" >&2
  exit 0
fi
codesign --verify --strict --deep "$app"

if ! xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  echo "signed, not notarised: no notarytool keychain profile \"$profile\"" >&2
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ditto -c -k --keepParent "$app" "$work/God.zip"
xcrun notarytool submit "$work/God.zip" --keychain-profile "$profile" --wait | tee "$work/notary.log"
grep "status: Accepted" "$work/notary.log" >/dev/null || { echo "notarisation refused" >&2; exit 1; }
xcrun stapler staple "$app"
xcrun stapler validate "$app"

# The zip is made after stapling, so the copy somebody unpacks carries its ticket and opens offline.
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

# As a download would arrive: unpacked from the zip, quarantined, and judged by Gatekeeper.
check="$work/check"
mkdir "$check"
ditto -x -k "$zip" "$check"
xattr -w com.apple.quarantine "0083;00000000;Safari;" "$check/God.app"
judged=$(spctl -a -vvv -t exec "$check/God.app" 2>&1 || true)
echo "$judged"
[[ "$judged" == *"source=Notarized Developer ID"* ]] || { echo "Gatekeeper would not open it" >&2; exit 1; }
echo "ok: $zip"

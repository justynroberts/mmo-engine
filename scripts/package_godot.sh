#!/usr/bin/env bash
# Package the Godot client as a macOS app for other people to play: a zip with an .app that opens on the join
# screen, the server field filled in with SERVER (default: this Mac's LAN address and the web port).
#   scripts/package_godot.sh [server]      -> dist/mmo-client-mac.zip
# Needs Godot 4.6 on the PATH and its macOS export template (Editor > Manage Export Templates). Apple silicon only.
set -euo pipefail
cd "$(dirname "$0")/.."
server="${1:-$(ipconfig getifaddr en0 2>/dev/null || echo 127.0.0.1)}"
scripts/build_godot.sh release
echo "$server" > godot/default_server.txt
trap 'rm -f godot/default_server.txt' EXIT
mkdir -p dist
rm -rf dist/mmo-client.zip dist/mmo-client-mac.zip
godot --headless --path godot --export-release macOS ../dist/mmo-client.zip
[ -f dist/mmo-client.zip ] || { echo "export failed (see above)" >&2; exit 1; }

# Sign with a Developer ID (hardened runtime) and notarise, so other Macs open it without complaint. Without the
# certificate it stays ad-hoc signed; without the notarytool keychain profile (NOTARY_KEYCHAIN_PROFILE, default
# "notarytool") it is signed but not notarised.
identity=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1)
if [ -n "$identity" ]; then
  work=$(mktemp -d)
  ditto -x -k dist/mmo-client.zip "$work"
  app=$(find "$work" -maxdepth 1 -name '*.app' | head -1)
  codesign --force --timestamp --options runtime --sign "$identity" "$app"/Contents/Frameworks/*.dylib
  codesign --force --timestamp --options runtime --sign "$identity" "$app"
  codesign --verify --strict --deep "$app"
  rm dist/mmo-client.zip
  ditto -c -k --keepParent "$app" dist/mmo-client.zip
  profile="${NOTARY_KEYCHAIN_PROFILE:-notarytool}"
  if xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
    xcrun notarytool submit dist/mmo-client.zip --keychain-profile "$profile" --wait | tee "$work/notary.log"
    grep -q "status: Accepted" "$work/notary.log" || { echo "notarisation failed" >&2; exit 1; }
    xcrun stapler staple "$app"
    rm dist/mmo-client.zip
    ditto -c -k --keepParent "$app" dist/mmo-client.zip
  else
    echo "signed as $identity, not notarised: no notarytool keychain profile \"$profile\"" >&2
  fi
  rm -rf "$work"
fi
mv dist/mmo-client.zip dist/mmo-client-mac.zip
echo "dist/mmo-client-mac.zip (server $server)"

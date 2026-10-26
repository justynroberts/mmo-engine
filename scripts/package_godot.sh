#!/usr/bin/env bash
# Package the Godot client as a macOS app for other people to play: a zip with an .app that opens on the join
# screen, the server field filled in with SERVER (default: the public server, hibernian.apps.fintonlabs.com).
#   scripts/package_godot.sh [server]      -> dist/mmo-client-mac.zip
# It also publishes the zip for the in-game updater: data/client/mac.zip and mac.json (version, size, sha256), which
# the server's web gateway serves at /play/client (CLIENT_DIR to put them elsewhere, CLIENT_DIR= to skip).
# Needs Godot 4.6 on the PATH and its macOS export template (Editor > Manage Export Templates). Apple silicon only.
set -euo pipefail
cd "$(dirname "$0")/.."
server="${1:-hibernian.apps.fintonlabs.com}"
scripts/build_godot.sh release
echo "$server" > godot/default_server.txt
# Versions are the packaging time, so they sort as text: an app offers an update when the server has a later one.
version=$(date -u +%Y%m%d.%H%M%S)
echo "$version" > godot/version.txt
trap 'rm -f godot/default_server.txt godot/version.txt' EXIT
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
echo "dist/mmo-client-mac.zip (server $server, version $version)"

client_dir="${CLIENT_DIR-data/client}"
if [ -n "$client_dir" ]; then
  mkdir -p "$client_dir"
  /bin/cp -f dist/mmo-client-mac.zip "$client_dir/mac.zip.new"
  /bin/mv -f "$client_dir/mac.zip.new" "$client_dir/mac.zip"
  size=$(stat -f %z "$client_dir/mac.zip")
  sha=$(shasum -a 256 "$client_dir/mac.zip" | cut -d' ' -f1)
  printf '{"version":"%s","size":%s,"sha256":"%s"}\n' "$version" "$size" "$sha" > "$client_dir/mac.json"
  echo "published for the updater: $client_dir/mac.json (version $version)"
fi

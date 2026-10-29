#!/usr/bin/env bash
# Package the Godot client for Windows (x86_64), cross-built from a Mac: a zip with the .exe, its .pck and the
# extension DLL, opening on the join screen with the server field filled in with SERVER (default: the public server).
#   scripts/package_windows.sh [server]      -> dist/mmo-client-windows.zip
# Needs Godot 4.6 on the PATH with its Windows export template (windows_release_x86_64.exe in the export templates),
# the x86_64-pc-windows-gnu Rust target (rustup target add x86_64-pc-windows-gnu) and MinGW (brew install mingw-w64).
# The DLL needs only Windows' own libraries (the UCRT, so Windows 10 or later). It is not code-signed, so
# SmartScreen asks once ("More info", "Run anyway").
set -euo pipefail
cd "$(dirname "$0")/.."
server="${1:-hibernian.apps.fintonlabs.com}"
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --release -p mmo-godot --target x86_64-pc-windows-gnu
mkdir -p godot/bin
cp -f target/x86_64-pc-windows-gnu/release/mmo_godot.dll godot/bin/
echo "$server" > godot/default_server.txt
version=$(date -u +%Y%m%d.%H%M%S)
echo "$version" > godot/version.txt
trap 'rm -f godot/default_server.txt godot/version.txt' EXIT
rm -rf dist/windows dist/mmo-client-windows.zip
mkdir -p dist/windows
godot --headless --path godot --export-release Windows ../dist/windows/mmo-client.exe
[ -f dist/windows/mmo-client.exe ] || { echo "export failed (see above)" >&2; exit 1; }
# Godot copies the extension's DLL beside the exe; make sure it is there.
[ -f dist/windows/mmo_godot.dll ] || cp -f godot/bin/mmo_godot.dll dist/windows/
(cd dist && mv windows Hibernian && zip -qr mmo-client-windows.zip Hibernian && mv Hibernian windows)
echo "dist/mmo-client-windows.zip (version $version, server $server)"

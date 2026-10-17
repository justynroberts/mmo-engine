#!/usr/bin/env bash
# Build the Godot extension and put it where the godot/ project looks for it.
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-debug}"
if [ "$profile" = release ]; then cargo build --release -p mmo-godot; else cargo build -p mmo-godot; fi
mkdir -p godot/bin
case "$(uname -s)" in
  Darwin) cp "target/$profile/libmmo_godot.dylib" godot/bin/ ;;
  MINGW*|MSYS*|CYGWIN*) cp "target/$profile/mmo_godot.dll" godot/bin/ ;;
  *) cp "target/$profile/libmmo_godot.so" godot/bin/ ;;
esac
echo "extension copied to godot/bin"

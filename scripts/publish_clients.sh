#!/usr/bin/env bash
# Put the packaged clients on a GitHub release, where the apps look for updates: dist/mmo-client-mac.zip and
# dist/mmo-client-windows.zip (from scripts/package_godot.sh and scripts/package_windows.sh) with their manifests
# mac.json and windows.json (version, size, sha256). Whichever of the two are there go up.
#   scripts/publish_clients.sh [--prerelease] [notes file]
# The release is tagged client-<newest version> on the commit checked out (pushed first). Needs gh, logged in.
set -euo pipefail
cd "$(dirname "$0")/.."
pre=""
if [ "${1:-}" = "--prerelease" ]; then pre="--prerelease"; shift; fi
notes="${1:-}"
assets=()
version=""
for p in mac windows; do
  if [ -f "dist/mmo-client-$p.zip" ] && [ -f "dist/$p.json" ]; then
    want=$(sed -n 's/.*"sha256":"\([0-9a-f]*\)".*/\1/p' "dist/$p.json")
    have=$(shasum -a 256 "dist/mmo-client-$p.zip" | cut -d' ' -f1)
    [ "$want" = "$have" ] || { echo "dist/$p.json does not match dist/mmo-client-$p.zip: package it again" >&2; exit 1; }
    assets+=("dist/mmo-client-$p.zip" "dist/$p.json")
    v=$(sed -n 's/.*"version":"\([^"]*\)".*/\1/p' "dist/$p.json")
    [[ "$v" > "$version" ]] && version="$v"
  fi
done
[ ${#assets[@]} -gt 0 ] || { echo "nothing to publish: package a client first" >&2; exit 1; }
tag="client-$version"
commit=$(git rev-parse HEAD)
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git tag "$tag" "$commit"
git push -q origin "$tag"
args=(--verify-tag --title "Clients $version" $pre)
if [ -n "$notes" ]; then args+=(--notes-file "$notes"); else args+=(--notes "Clients built from $commit."); fi
gh release create "$tag" "${assets[@]}" "${args[@]}"

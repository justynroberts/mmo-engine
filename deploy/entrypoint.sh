#!/bin/sh
# Start the server in /data. A fresh volume is seeded with the game and world shipped in the image, never
# overwriting what is already there, except the game file when the seed's VERSION moves on; MMO_ARGS adds flags for
# this deployment (behind a proxy: --web 0.0.0.0:7790 --web-trust-proxy).
#
# MMO_SEED picks the seed: unset (or "seed") is deploy/seed, any other name is deploy/<name> (shipped as
# /opt/seeds/<name>). A seed has game.json and either world.json or recipe.json, from which worldgen makes the world
# on first start (minutes, for a big one). Changing MMO_SEED on a volume that already has a world is a new world:
# the old world, its built blocks and its game file are moved aside into /data/before-<seed>-<time>/, and the first
# run starts with --fresh-world (no saved bots, plots, guilds, auction or mail from the old map). Characters, their
# levels, bags and quests are kept; anyone whose saved spot is not on the new map comes in at its start place.
set -e
cd /data
name=${MMO_SEED:-seed}
if [ "$name" = "seed" ]; then seed=/opt/seed; else seed=/opt/seeds/$name; fi
[ -d "$seed" ] || { echo "no seed called $name"; exit 1; }
fresh=""
# The seed whose world is here (a volume from before seeds had names holds the default one).
here=$(cat /data/.world-seed 2>/dev/null || true)
[ -z "$here" ] && [ -e /data/world.json ] && here=seed
if [ -n "$here" ] && [ "$here" != "$name" ]; then
    aside="/data/before-$here-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$aside"
    for f in world.json world.blocks.d world.blocks.json game.json .seed-version; do
        [ -e "/data/$f" ] && mv "/data/$f" "$aside/"
    done
    echo "new world ($here -> $name): the old one is in $aside"
    # Kept until the server is started on the new world, so a restart part way through (worldgen stopped) still
    # starts it fresh.
    touch /data/.fresh-world
fi
[ -e /data/.fresh-world ] && fresh="--fresh-world"
for f in game.json world.json world.blocks.d; do
    if [ ! -e "/data/$f" ] && [ -e "$seed/$f" ]; then
        cp -R "$seed/$f" "/data/$f"
        echo "seeded $f"
    fi
done
if [ ! -e /data/world.json ] && [ -e "$seed/recipe.json" ]; then
    echo "making the world from $name's recipe (this takes a while)"
    worldgen --recipe "$seed/recipe.json" --out /data/world.json.part
    mv /data/world.json.part /data/world.json
fi
echo "$name" > /data/.world-seed
# A deployment with no reachable admin console takes its game file from the seed: when the seed's version
# (VERSION) differs from the one last applied, the seed's game.json replaces the volume's, the old one kept beside
# it. The world, built blocks and characters are never touched.
if [ -e "$seed/VERSION" ] && [ -e "$seed/game.json" ]; then
    want=$(cat "$seed/VERSION")
    have=$(cat /data/.seed-version 2>/dev/null || true)
    if [ "$want" != "$have" ]; then
        [ -e /data/game.json ] && cp /data/game.json "/data/game.json.before-$want"
        cp "$seed/game.json" /data/game.json
        echo "$want" > /data/.seed-version
        echo "game.json from seed $want"
    fi
fi
rm -f /data/.fresh-world
if [ "$1" = "server" ] && [ -n "$MMO_ARGS$fresh" ]; then
    # shellcheck disable=SC2086
    exec "$@" $MMO_ARGS $fresh
fi
exec "$@"

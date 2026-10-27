#!/bin/sh
# Start the server in /data. A fresh volume is seeded with the game and world shipped in the image
# (deploy/seed), never overwriting what is already there, except the game file when the seed's VERSION moves on; MMO_ARGS adds flags for this deployment
# (behind a proxy: --web 0.0.0.0:7790 --web-trust-proxy).
set -e
cd /data
for f in game.json world.json world.blocks.d; do
    if [ ! -e "/data/$f" ] && [ -e "/opt/seed/$f" ]; then
        cp -R "/opt/seed/$f" "/data/$f"
        echo "seeded $f"
    fi
done
# A deployment with no reachable admin console takes its game file from the seed: when the seed's version
# (deploy/seed/VERSION) differs from the one last applied, the seed's game.json replaces the volume's, the old one
# kept beside it. The world, built blocks and characters are never touched.
if [ -e /opt/seed/VERSION ] && [ -e /opt/seed/game.json ]; then
    want=$(cat /opt/seed/VERSION)
    have=$(cat /data/.seed-version 2>/dev/null || true)
    if [ "$want" != "$have" ]; then
        [ -e /data/game.json ] && cp /data/game.json "/data/game.json.before-$want"
        cp /opt/seed/game.json /data/game.json
        echo "$want" > /data/.seed-version
        echo "game.json from seed $want"
    fi
fi
if [ "$1" = "server" ] && [ -n "$MMO_ARGS" ]; then
    # shellcheck disable=SC2086
    exec "$@" $MMO_ARGS
fi
exec "$@"

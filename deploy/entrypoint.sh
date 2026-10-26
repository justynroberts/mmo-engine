#!/bin/sh
# Start the server in /data. A fresh volume is seeded with the game and world shipped in the image
# (deploy/seed), never overwriting what is already there; MMO_ARGS adds flags for this deployment
# (behind a proxy: --web 0.0.0.0:7790 --web-trust-proxy).
set -e
cd /data
for f in game.json world.json world.blocks.d; do
    if [ ! -e "/data/$f" ] && [ -e "/opt/seed/$f" ]; then
        cp -R "/opt/seed/$f" "/data/$f"
        echo "seeded $f"
    fi
done
if [ "$1" = "server" ] && [ -n "$MMO_ARGS" ]; then
    # shellcheck disable=SC2086
    exec "$@" $MMO_ARGS
fi
exec "$@"

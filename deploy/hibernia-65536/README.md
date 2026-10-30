# Hibernia at 65,536

The seed world (`deploy/seed`, 4,096 units) made sixteen times bigger: the same coast, rivers, lakes, mountains and
named towns, with the country between filled in (about 1,300 villages, farmsteads, 31,000 fields with hedgerows, and
roads in tiers: highways, roads, lanes and farm tracks). Standing stones in each town lead to the capital and back.

Both files here come from `scripts/hibernia_big.py`; the world file is too big to keep in git (about 320 MB), so it
is made from the recipe:

```sh
cargo build --release -p worldgen
target/release/worldgen --recipe deploy/hibernia-65536/recipe.json --out world.json     # about 5 minutes, 800 MB
scripts/hibernia_big.py game --world world.json > deploy/hibernia-65536/game.json      # only after changing the script
server --world world.json --game deploy/hibernia-65536/game.json
```

Measured (debug server in Docker): starts in about 35 s, 526 MB resident with its 951 bots.

Moving the live server to it is a new world: built blocks, plots and saved positions belong to the old map. The
deploy entrypoint never replaces an existing `world.json`, so it needs a fresh volume (or the old world moved aside).

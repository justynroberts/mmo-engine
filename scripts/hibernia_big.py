#!/usr/bin/env python3
"""Hibernia at a bigger size: the same land, towns, rivers, lakes and mountains as the 4,096 unit seed world,
scaled up (16 times for 65,536), with the space between filled by the generator.

  scripts/hibernia_big.py recipe [--size 65536] [--from deploy/seed] > recipe.json
  target/release/worldgen --recipe recipe.json --out world.json
  scripts/hibernia_big.py game --world world.json [--from deploy/seed] > game.json

`recipe` scales the seed world's recipe: the coast and river courses (with detail added between their points, so
they are not long straight lines at the new size), the named settlements, the lakes, and each hand-placed mountain
(a crag) turned into a range of crags over the ground it now covers. `game` moves the game file onto the new world:
a place that is a town follows its town to where the generator put it (and keeps its size); a place near a town
keeps its offset from it; anything else (lakes, mountains, the north) is scaled. Routes are scaled. Standing stones
are added: one in each town that leads to the capital, and a ring in the capital that leads to each town.
"""

import argparse
import json
import math
import random
import sys

SEED_DIR = "deploy/seed"


def load(path):
    with open(path) as f:
        return json.load(f)


def roughen(pts, scale, rng, closed, step, jitter):
    """Scale a line and add detail: each segment is split until its pieces are under `step` units, each new point
    pushed sideways by up to `jitter` of the piece's length."""
    pts = [(x * scale, y * scale) for x, y in pts]
    out = []
    n = len(pts)
    segs = n if closed else n - 1
    for k in range(segs):
        a, b = pts[k], pts[(k + 1) % n]
        piece = [a, b]
        while max(math.dist(piece[i], piece[i + 1]) for i in range(len(piece) - 1)) > step:
            nxt = [piece[0]]
            for i in range(len(piece) - 1):
                p, q = piece[i], piece[i + 1]
                d = math.dist(p, q)
                mx, my = (p[0] + q[0]) / 2, (p[1] + q[1]) / 2
                if d > 1e-6:
                    nx, ny = -(q[1] - p[1]) / d, (q[0] - p[0]) / d
                    off = rng.uniform(-jitter, jitter) * d
                    mx, my = mx + nx * off, my + ny * off
                nxt += [(mx, my), q]
            piece = nxt
        out += piece[:-1]
    if not closed:
        out.append(pts[-1])
    return [[round(x, 1), round(y, 1)] for x, y in out]


def big_recipe(seed_dir, size):
    w = load(f"{seed_dir}/world.json")
    r = w["recipe"]
    old = r["size"]
    k = size / old
    rng = random.Random(4096)
    out = {
        "size": size,
        "seed": r["seed"],
        "city": True,
        "roads": True,
        "hills": r.get("hills", 1.0),
        # Usual counts are per area: these dials keep a hamlet within a minute or two of anywhere on the land, not
        # one every few hundred units, and the woods thinner (the walls stay well under the engine's 4 million).
        "villages": 0.12,
        "forts": 0.06,
        "crags": 0.25,
        "lakes": 0.25,
        "forest": 1.2,
        # The seed's rivers, scaled; the generator adds none of its own.
        "rivers": 0,
    }
    out["coast"] = [roughen(o, k, rng, True, 140.0, 0.16) for o in r["coast"]]
    out["river_courses"] = [roughen(c, k, rng, False, 160.0, 0.12) for c in r["river_courses"]]
    sett = []
    for s in r["settlements"]:
        t = dict(s)
        t["at"] = [round(s["at"][0] * k, 1), round(s["at"][1] * k, 1)]
        if s["t"] == "city":
            t["side"] = 600.0  # the capital, as wide as the generator makes one
        sett.append(t)
    out["settlements"] = sett
    edits = []
    for e in r.get("edits", []):
        a = e.get("add")
        if not a:
            continue
        b = [v * k for v in a["b"]]
        if a["t"] == "lake":
            # A lake: not the box it was drawn as (a straight shore thousands of units long) but strips across it,
            # each as wide as an ellipse with a ragged edge is there, touching so the water runs on.
            w_, h_ = b[2] - b[0], b[3] - b[1]
            along = h_ >= w_
            length, across = (h_, w_) if along else (w_, h_)
            n = max(4, min(40, int(length / 200.0)))
            mid = across / 2
            drift = 0.0
            for i in range(n):
                t0, t1 = i / n, (i + 1) / n
                tc = (t0 + t1) / 2 * 2 - 1
                half = mid * math.sqrt(max(0.0, 1 - tc * tc)) * rng.uniform(0.7, 1.0)
                half = max(half, min(mid, 30.0))
                drift = max(-mid * 0.25, min(mid * 0.25, drift + rng.uniform(-0.08, 0.08) * mid))
                c = mid + drift
                lo, hi = max(0.0, c - half), min(across, c + half)
                if along:
                    box = [b[0] + lo, b[1] + t0 * length, b[0] + hi, b[1] + t1 * length]
                else:
                    box = [b[0] + t0 * length, b[1] + lo, b[0] + t1 * length, b[1] + hi]
                edits.append({"add": {"t": "lake", "b": [round(v, 1) for v in box]}})
        elif a["t"] == "crag":
            # A mountain: not one crag the size of a county, but a range of them over that ground, some way apart,
            # with passes between.
            w_, h_ = b[2] - b[0], b[3] - b[1]
            n = max(6, min(90, int(w_ * h_ / (170.0 * 170.0))))
            for _ in range(n):
                cw, ch = rng.uniform(26, 70), rng.uniform(26, 70)
                cx, cy = rng.uniform(b[0] + cw, b[2] - cw), rng.uniform(b[1] + ch, b[3] - ch)
                edits.append({"add": {"t": "crag", "b": [round(cx - cw / 2, 1), round(cy - ch / 2, 1), round(cx + cw / 2, 1), round(cy + ch / 2, 1)]}})
    out["edits"] = edits
    return out


def objects_of(world_path):
    """Squares (open ground in towns) from a world file, read as a stream (the file may be large)."""
    squares = []
    settled = []
    head = None
    with open(world_path) as f:
        text = f.read()
    w = json.loads(text)
    for o in w.get("objects", []):
        if o.get("t") == "square":
            b = o["b"]
            squares.append(((b[0] + b[2]) / 2, (b[1] + b[3]) / 2, min(b[2] - b[0], b[3] - b[1]) / 2))
    settled = w.get("settled", [])
    return w["size"], settled, squares


def big_game(seed_dir, world_path):
    g = load(f"{seed_dir}/game.json")
    ow = load(f"{seed_dir}/world.json")
    size, settled, squares = objects_of(world_path)
    k = size / ow["size"]
    old_at = {s["name"]: s["at"] for s in ow["settled"] if s.get("at") and s.get("name")}
    new_at = {s["name"]: s["at"] for s in settled if s.get("at") and s.get("name")}
    kind = {s["name"]: s["t"] for s in settled if s.get("name")}

    def town_of(x, y, reach):
        best = None
        for n, (ax, ay) in old_at.items():
            d = math.hypot(x - ax, y - ay)
            if d <= reach and n in new_at and (best is None or d < best[0]):
                best = (d, n)
        return best[1] if best else None

    def move(x, y, reach=160.0):
        """A point on the old map to the new one: beside a town it keeps its offset from the town, else scaled."""
        n = town_of(x, y, reach)
        if n:
            return new_at[n][0] + (x - old_at[n][0]), new_at[n][1] + (y - old_at[n][1]), True
        return x * k, y * k, False

    clamp = lambda v: max(1.0, min(size - 1.0, v))
    for p in g["places"]:
        if p.get("title") in new_at:
            n = p["title"]
            p["x"], p["y"] = new_at[n][0] + (p["x"] - old_at[n][0]), new_at[n][1] + (p["y"] - old_at[n][1])
        else:
            x, y, near = move(p["x"], p["y"], 160.0 if p["r"] <= 40 else 0.0)
            p["x"], p["y"] = x, y
            if not near:
                p["r"] = min(8192.0, p["r"] * k) if p["r"] >= 400 else min(2048.0, p["r"] * k * 0.5)
        p["x"], p["y"] = round(clamp(p["x"]), 1), round(clamp(p["y"]), 1)
        p["r"] = round(p["r"], 1)
        if p.get("exit") and not p["exit"].get("zone"):
            x, y, _ = move(p["exit"]["x"], p["exit"]["y"])
            p["exit"]["x"], p["exit"]["y"] = round(clamp(x), 1), round(clamp(y), 1)
    for r in g.get("routes", []):
        pts = r["points"]
        inside = all(town_of(x, y, 160.0) for x, y in pts)
        r["points"] = [[round(clamp(a), 1), round(clamp(b), 1)] for a, b, _ in (move(x, y, 160.0 if inside else 0.0) for x, y in pts)]

    # Wildlife: what roams the whole land (no home) is as thick on the ground as before, so there is k * k times as
    # much of it, held to sixteen times; what lives in a wild place that has grown (a forest, the north) four times.
    wide = {p["name"] for p in g["places"] if p["r"] >= 500}
    for a in g["archetypes"]:
        n = a.get("count", 0)
        if n == 0 or a.get("homes"):
            continue
        if not a.get("home"):
            a["count"] = int(n * min(k * k, 16))
        elif a["home"] in wide:
            a["count"] = int(n * 4)

    # Standing stones: in each town, by its square, one that takes you to the capital; in the capital a ring of
    # them, one to each town. You arrive beside the far stone, not in it.
    def square_near(x, y):
        best = min(squares, key=lambda s: math.hypot(s[0] - x, s[1] - y), default=None)
        if best and math.hypot(best[0] - x, best[1] - y) < 400:
            return best
        return (x, y, 8.0)

    capital = next((n for n, t in kind.items() if t == "city"), None)
    towns = [n for n, t in kind.items() if t == "town"]
    by_title = {p.get("title"): p for p in g["places"]}
    names = {p["name"] for p in g["places"]}
    if capital and capital in new_at:
        cx, cy, cr = square_near(*new_at[capital])
        ring = max(10.0, min(cr - 4.0, 18.0))
        for i, t in enumerate(towns):
            tx, ty, tr = square_near(*new_at[t])
            ang = i / len(towns) * math.tau
            sx, sy = cx + math.cos(ang) * ring, cy + math.sin(ang) * ring
            town_place = by_title.get(t)
            label = town_place["name"] if town_place else t.lower()
            stone_c = f"cloch {label}"[:32]
            stone_t = f"cloch {label} home"[:32]
            # The town's stone, back to the capital.
            g["places"].append({"name": stone_t, "x": round(tx + 6.0, 1), "y": round(ty, 1), "r": 2.5, "title": f"Cloch go {capital}"[:40],
                                "exit": {"x": round(sx + math.cos(ang) * 6.0, 1), "y": round(sy + math.sin(ang) * 6.0, 1)}})
            # The capital's stone, out to the town.
            g["places"].append({"name": stone_c, "x": round(sx, 1), "y": round(sy, 1), "r": 2.5, "title": f"Cloch go {t}"[:40],
                                "exit": {"x": round(tx + 12.0, 1), "y": round(ty, 1)}})
            names |= {stone_t, stone_c}
    # New characters, and those whose saved spot was on the old map, come into the world at the capital.
    cap = by_title.get(capital) if capital else None
    if cap:
        g["start_place"] = cap["name"]
    return g


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("what", choices=["recipe", "game"])
    ap.add_argument("--size", type=float, default=65536.0)
    ap.add_argument("--from", dest="src", default=SEED_DIR)
    ap.add_argument("--world")
    a = ap.parse_args()
    if a.what == "recipe":
        json.dump(big_recipe(a.src, a.size), sys.stdout)
    else:
        if not a.world:
            ap.error("game needs --world")
        json.dump(big_game(a.src, a.world), sys.stdout, ensure_ascii=False, indent=1)


if __name__ == "__main__":
    main()

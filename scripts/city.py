#!/usr/bin/env python3
"""Lay a walled city into a running world, through the admin API.

Usage: scripts/city.py [--admin 127.0.0.1:7780] [--size 360] [--at X,Y] [--dry-run]

The city is ordinary server walls (hand-placed walls in the game file), so it blocks movement exactly where it
stands and applies live without dropping anyone. The world's other walls are kept, except those inside the
city's footprint. The city is recorded as the place "city"; running the script again clears that footprint and
rebuilds there (or at --at / with --size), so it can be rerun to move or resize it without leaving a copy.

What clients draw for each wall box comes from its corner coordinates (godot/scripts/terrain.gd box_kind and
main.gd's house rule), so each box here is nudged a unit or two until it comes out as what it should be:
houses along the streets, keeps for the walls, towers and the castle.

The layout: a square curtain wall with towers and four gates, avenues from the gates to a central plaza with
a castle, side streets, and blocks of terraced houses back to back along every street, one block left open as
a market square. The game's "plaza" and "market" places move into the city, so the merchants and guards that
live there follow.
"""
import argparse
import json
import sys
import urllib.request

WALL = 3.0        # curtain wall thickness
GATE = 18.0       # gate opening
TOWER = 9.0       # towers stay under 10 units, so they are always keeps
ROW = 12.0        # depth of a row of houses
ALLEY = 1.0       # gap between back-to-back rows
STREET = 10.0
AVENUE = 16.0
BLOCK = 46.0      # length of a block of houses
RING = 12.0       # road inside the curtain wall


def api(base, path, method="GET", body=None):
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"X-Admin": "1", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"{}")


# ------------------------------------------------------------------ how clients will see a box

def kind(b):
    """terrain.gd box_kind: 0 keep, 1 crag, 2 lake."""
    w, d = b[2] - b[0], b[3] - b[1]
    if min(w, d) < 10:
        return 0
    h = (int(b[0]) * 7 + int(b[1]) * 13) % 10
    if h >= 8 and min(w, d) >= 12:
        return 2
    return 1 if h >= 5 else 0


def is_house(b):
    """main.gd: a keep becomes houses when its short side is 6 to 26 and this parity is even."""
    w, d = b[2] - b[0], b[3] - b[1]
    return kind(b) == 0 and 6 <= min(w, d) <= 26 and (int(b[0]) * 3 + int(b[1]) * 5) % 2 == 0


def nudge(b, want_house):
    """Shift a box by up to 3 units so it comes out as a house, or as a keep that is not a house."""
    for dx in range(4):
        for dy in range(4):
            c = [b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy]
            if want_house and is_house(c):
                return c
            if not want_house and kind(c) == 0 and not is_house(c):
                return c
    return b


# ------------------------------------------------------------------ layout

def layout(cx, cy, size):
    h = size / 2
    x0, y0, x1, y1 = cx - h, cy - h, cx + h, cy + h
    keeps, houses = [], []
    # Curtain wall, broken by a gate in the middle of each side.
    for (ax, ay, bx, by, horiz) in ((x0, y0, x1, y0, True), (x0, y1, x1, y1, True), (x0, y0, x0, y1, False), (x1, y0, x1, y1, False)):
        if horiz:
            m = (ax + bx) / 2
            keeps += [[ax, ay - WALL / 2, m - GATE / 2, ay + WALL / 2], [m + GATE / 2, ay - WALL / 2, bx, ay + WALL / 2]]
        else:
            m = (ay + by) / 2
            keeps += [[ax - WALL / 2, ay, ax + WALL / 2, m - GATE / 2], [ax - WALL / 2, m + GATE / 2, ax + WALL / 2, by]]
    # Towers: the corners, both sides of each gate, and along the walls every 70 units or so.
    t = TOWER / 2
    spots = [(x0, y0), (x1, y0), (x0, y1), (x1, y1),
             (cx - GATE / 2 - t, y0), (cx + GATE / 2 + t, y0), (cx - GATE / 2 - t, y1), (cx + GATE / 2 + t, y1),
             (x0, cy - GATE / 2 - t), (x0, cy + GATE / 2 + t), (x1, cy - GATE / 2 - t), (x1, cy + GATE / 2 + t)]
    n = max(1, int((h - GATE) / 70))
    for k in range(1, n + 1):
        f = k / (n + 1)
        for s in (-1, 1):
            spots += [(cx + s * (GATE / 2 + f * (h - GATE / 2)), y0), (cx + s * (GATE / 2 + f * (h - GATE / 2)), y1),
                      (x0, cy + s * (GATE / 2 + f * (h - GATE / 2))), (x1, cy + s * (GATE / 2 + f * (h - GATE / 2)))]
    keeps += [[sx - t, sy - t, sx + t, sy + t] for sx, sy in spots]
    # The castle in the middle of the plaza: a great keep with a tower at each corner.
    plaza = max(70.0, size * 0.24)
    keeps.append([cx - 18, cy - 14, cx + 18, cy + 14])
    keeps += [[cx + sx * 22 - t, cy + sy * 18 - t, cx + sx * 22 + t, cy + sy * 18 + t] for sx in (-1, 1) for sy in (-1, 1)]
    # Quarters between the avenues: rows of houses back to back, blocks separated by streets.
    inner = h - WALL / 2 - RING
    market = None
    for qx in (-1, 1):
        for qy in (-1, 1):
            lo_x, hi_x = sorted((cx + qx * AVENUE / 2, cx + qx * inner))
            lo_y, hi_y = sorted((cy + qy * AVENUE / 2, cy + qy * inner))
            y = lo_y
            while y + 2 * ROW + ALLEY <= hi_y:
                x = lo_x
                while x + BLOCK <= hi_x:
                    blk = (x, y, x + BLOCK, y + 2 * ROW + ALLEY)
                    near_plaza = abs((blk[0] + blk[2]) / 2 - cx) < plaza / 2 + BLOCK / 2 and abs((blk[1] + blk[3]) / 2 - cy) < plaza / 2 + ROW
                    if not near_plaza:
                        if market is None and qx == 1 and qy == 1:
                            market = ((blk[0] + blk[2]) / 2, (blk[1] + blk[3]) / 2)  # left open: the market square
                        else:
                            houses += [[blk[0], blk[1], blk[2], blk[1] + ROW], [blk[0], blk[1] + ROW + ALLEY, blk[2], blk[3]]]
                    x += BLOCK + STREET
                y += 2 * ROW + ALLEY + STREET
    keeps = [nudge(b, False) for b in keeps]
    houses = [nudge(b, True) for b in houses]
    return keeps, houses, (x0 - 6, y0 - 6, x1 + 6, y1 + 6), market or (cx + inner / 2, cy + inner / 2)


def overlaps(b, r):
    return b[0] < r[2] and b[2] > r[0] and b[1] < r[3] and b[3] > r[1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--admin", default="127.0.0.1:7780")
    ap.add_argument("--size", type=float, default=0, help="side of the city in units (default: a sixth of the world, 200 to 600)")
    ap.add_argument("--at", default="", help="centre X,Y (default: the emptiest spot that fits)")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    base = "http://" + a.admin
    s, game = api(base, "/api/game")
    s2, world = api(base, "/api/world")
    if s != 200 or s2 != 200:
        sys.exit(f"admin API at {base} did not answer ({s}, {s2})")
    wsize = world["size"]
    size = a.size or max(200.0, min(600.0, round(wsize / 6 / 10) * 10))
    existing = [list(b) for b in world["boxes"]]
    # A city from an earlier run is recorded as the place "city": clear its footprint, and build there again
    # unless told otherwise.
    old = next((p for p in game.get("places", []) if p["name"] == "city"), None)
    if old:
        oh = old["r"] + 6
        existing = [b for b in existing if not overlaps(b, (old["x"] - oh, old["y"] - oh, old["x"] + oh, old["y"] + oh))]
    if a.at:
        cx, cy = (float(v) for v in a.at.split(","))
    elif old and not a.size:
        cx, cy, size = old["x"], old["y"], old["r"] * 2
    else:
        # The emptiest spot that fits, away from the edge.
        best = None
        step = size / 4
        cyv = size / 2 + 40
        while cyv <= wsize - size / 2 - 40:
            cxv = size / 2 + 40
            while cxv <= wsize - size / 2 - 40:
                r = (cxv - size / 2, cyv - size / 2, cxv + size / 2, cyv + size / 2)
                n = sum(overlaps(b, r) for b in existing)
                if best is None or n < best[0]:
                    best = (n, cxv, cyv)
                cxv += step
            cyv += step
        if best is None:
            sys.exit(f"a {size:.0f} unit city does not fit a {wsize:.0f} unit world")
        _, cx, cy = best
    keeps, houses, foot, market = layout(cx, cy, size)
    kept = [b for b in existing if not overlaps(b, foot)]
    cleared = len(world["boxes"]) - len(kept)
    walls = kept + keeps + houses
    print(f"city {size:.0f} units at {cx:.0f},{cy:.0f}: {len(houses)} rows of houses, {len(keeps)} walls, towers and keep; "
          f"{cleared} old walls cleared (an earlier city included), {len(walls)} walls in all")
    if len(walls) > 2000:
        sys.exit("more than 2000 walls: try a smaller --size")
    places = [p for p in game.get("places", []) if p["name"] not in ("plaza", "market", "city")]
    places.append({"name": "city", "x": round(cx, 1), "y": round(cy, 1), "r": round(size / 2, 1)})
    places += [{"name": "plaza", "x": round(cx, 1), "y": round(cy + 24, 1), "r": 18.0},
               {"name": "market", "x": round(market[0], 1), "y": round(market[1], 1), "r": 16.0}]
    game["walls"] = [[round(v, 2) for v in b] for b in walls]
    game["places"] = places
    if a.dry_run:
        return
    s, r = api(base, "/api/game", "PUT", game)
    if s != 200:
        sys.exit(f"the server refused it: {s} {r}")
    print("applied:", {k: r[k] for k in ("reset", "dropped_clients", "moved_entities") if k in r})


if __name__ == "__main__":
    main()

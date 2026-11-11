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

  scripts/hibernia_big.py country --world world.json --game deploy/hibernia-65536/game.json

`country` (also part of `game`) peoples the country between the towns, editing the game file in place: farmsteads
near the towns get a place of their own and a smallholder each, and villages along the highways an inn (a place
named for it, an innkeeper who sells food, drink and a horse, so a traveller has a market there, and villagers).
Run again, it replaces what it added before.

  scripts/hibernia_big.py villages --world world.json --game deploy/hibernia-65536/game.json

`villages` gives every village square out of the named places a place of its own (named from where it lies, as
`country` names them) and folk: villagers in each, and children, dogs and smallholders in some; sheep and shepherds
round the farmsteads; wild boar in the country; and more deer, foxes, stags, wild horses, wolves, travellers on the
roads and brigands. Run again, it replaces what it added before and sets the counts afresh.
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
                # Each strip runs on a little into the next, so the water is one (worldgen keeps the deepest bed).
                a0, a1 = t0 * length - (12.0 if i > 0 else 0.0), t1 * length + (12.0 if i < n - 1 else 0.0)
                if along:
                    box = [b[0] + lo, b[1] + a0, b[0] + hi, b[1] + a1]
                else:
                    box = [b[0] + a0, b[1] + lo, b[0] + a1, b[1] + hi]
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
    return w["size"], settled, squares, w


FARM = "feirm "
INN = "teach osta "
FARMER = "feirmeoir"
INNKEEPER = "ostoir"


def add_country(g, w, farms=96, inns=24):
    """Farmsteads and highway inns, with folk: see the module's doc. `w` is the world file."""
    g["places"] = [p for p in g["places"] if not p["name"].startswith((FARM, INN))]
    g["archetypes"] = [a for a in g["archetypes"] if a["name"] not in (FARMER, INNKEEPER)]
    arch = {a["name"]: a for a in g["archetypes"]}
    for a in g["archetypes"]:
        if a.get("homes"):
            ours = [h for h in a["homes"] if h.startswith(INN)]
            a["homes"] = [h for h in a["homes"] if not h.startswith((FARM, INN))]
            if a["name"] == "villager":
                a["count"] = a.get("count", 0) - 2 * len(ours)  # the two each inn was given
    houses = [o["b"] for o in w.get("objects", []) if o.get("t") == "house"]
    # The villages are the squares out of the named places; the world file names none of them, so each is called
    # something from where it lies (the same every run).
    first = ["Baile", "Cill", "Ráth", "Dún", "Áth", "Cluain", "Lios", "Tulach", "Achadh", "Gort", "Doire", "Mainistir"]
    second = ["an Mhuilinn", "na Coille", "Mór", "Beag", "an Droichid", "na hAbhann", "Bhuí", "Glas", "na gCloch",
              "an Chnoic", "Rua", "an Iarainn", "na Móna", "an Fhia", "Locha", "na Sceiche"]
    villages = []
    for o in w.get("objects", []):
        if o.get("t") != "square":
            continue
        b = o["b"]
        x, y = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        if any(math.hypot(x - p["x"], y - p["y"]) <= p["r"] + 100 for p in g["places"]):
            continue
        h = (int(x) * 73856093) ^ (int(y) * 19349663)
        villages.append((x, y, f"{first[h % len(first)]} {second[(h // len(first)) % len(second)]}"))
    towns = [(p["x"], p["y"], p["r"]) for p in g["places"] if p["r"] >= 60 and p.get("title") and not p.get("exit")]
    room = 256 - len(g["places"])
    farms, inns = min(farms, room * 4 // 5), min(inns, room // 5)
    taken = {p["name"] for p in g["places"]}

    def in_place(x, y, slack):
        return any(math.hypot(x - p["x"], y - p["y"]) <= p["r"] + slack for p in g["places"])

    def unique(base):
        base = base[:32].rstrip()
        n, name = 2, base
        while name in taken:
            tail = f" {n}"
            name, n = base[:32 - len(tail)].rstrip() + tail, n + 1
        taken.add(name)
        return name

    def plain(text):
        """A place name: lower case, no marks (as the game's other names are)."""
        import unicodedata
        return "".join(c for c in unicodedata.normalize("NFD", text.lower()) if unicodedata.category(c) != "Mn")

    # Inns: villages beside a highway (a long road as wide as a street), out of the towns, well apart.
    highway = []
    for r in w.get("roads", []):
        pts = r.get("pts", [])
        if abs(float(r.get("half_width", 3.0)) - 3.5) > 0.05 or len(pts) < 2:
            continue
        if sum(math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(pts, pts[1:])) < 1500:
            continue
        for a, b in zip(pts, pts[1:]):
            n = max(1, int(math.hypot(b[0] - a[0], b[1] - a[1]) // 40))
            highway += [(a[0] + (b[0] - a[0]) * t / n, a[1] + (b[1] - a[1]) * t / n) for t in range(n)]
    cell = 256.0
    hgrid = {}
    for x, y in highway:
        hgrid.setdefault((int(x // cell), int(y // cell)), []).append((x, y))

    def near_highway(x, y, reach):
        cx, cy = int(x // cell), int(y // cell)
        return any(math.hypot(x - hx, y - hy) <= reach for dx in (-1, 0, 1) for dy in (-1, 0, 1) for hx, hy in hgrid.get((cx + dx, cy + dy), []))

    near_town = lambda x, y: min((math.hypot(x - tx, y - ty) - tr for tx, ty, tr in towns), default=1e9)
    picked = []
    for x, y, name in sorted(villages, key=lambda v: near_town(v[0], v[1])):
        if len(picked) >= inns:
            break
        if near_town(x, y) < 1000 or in_place(x, y, 200) or not near_highway(x, y, 120):
            continue
        if any(math.hypot(x - px, y - py) < 2200 for px, py, _ in picked):
            continue
        picked.append((x, y, name))
    inn_places = []
    for x, y, name in picked:
        r = 55.0
        inn_places.append({"name": unique(INN + plain(name)), "x": round(x, 1), "y": round(y, 1), "r": r, "title": f"{name} (An Teach Ósta)"[:40], "music": "tavern"})

    # Farmsteads: houses out of every place and village, a farmhouse and its barn taken as one, near the towns first.
    village_at = [(x, y) for x, y, _ in villages]
    vgrid = {}
    for x, y in village_at:
        vgrid.setdefault((int(x // 512), int(y // 512)), []).append((x, y))

    def by_village(x, y):
        cx, cy = int(x // 512), int(y // 512)
        return any(math.hypot(x - vx, y - vy) < 260 for dx in (-1, 0, 1) for dy in (-1, 0, 1) for vx, vy in vgrid.get((cx + dx, cy + dy), []))

    steads = []
    for b in houses:
        x, y = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        if (b[2] - b[0]) * (b[3] - b[1]) < 40 or by_village(x, y) or in_place(x, y, 80):
            continue
        steads.append((x, y))
    farm_at = []
    for x, y in sorted(steads, key=lambda p: near_town(p[0], p[1])):
        if len(farm_at) >= farms:
            break
        if near_town(x, y) < 250 or any(math.hypot(x - fx, y - fy) < 1100 for fx, fy in farm_at):
            continue
        if any(math.hypot(x - p["x"], y - p["y"]) < 400 for p in inn_places):
            continue
        farm_at.append((x, y))

    def village_name(x, y):
        best = min(villages, key=lambda v: math.hypot(x - v[0], y - v[1]), default=None)
        return best[2] if best else "na tuaithe"

    farm_places = [{"name": unique(FARM + plain(village_name(x, y))), "x": round(x, 1), "y": round(y, 1), "r": 40.0} for x, y in farm_at]
    g["places"] += inn_places + farm_places

    flee = lambda a: [r for r in a.get("rules", []) if r["then"]["kind"] in ("flee", "indoors")]
    if farm_places and "farmer" in arch:
        f = {k: v for k, v in arch["farmer"].items() if k not in ("homes",)}
        f.update({"name": FARMER, "title": "Smallholder", "home": farm_places[0]["name"], "homes": [p["name"] for p in farm_places[1:]],
                  "count": len(farm_places), "work": "fields"})
        f["rules"] = flee(arch["farmer"]) + [
            {"when": {"kind": "during", "parts": ["dawn"]}, "then": {"kind": "chat", "lines": ["The cows are bawling for me already.", "Mist on the low field again."]}},
            {"when": {"kind": "during", "parts": ["day"]}, "then": {"kind": "chat", "lines": ["It's a long way to market from here.", "Mind the gate, it sticks.", "Not many come up this boreen.", "The hay won't save itself."]}},
            {"when": {"kind": "sees", "who": "player", "within": 12.0}, "then": {"kind": "chat", "lines": [
                "A traveller! You're a long way from the town.", "Stay on the road after dark, friend.", "There's an inn on the highway, if you're tired.", "God bless the work."]}},
        ]
        g["archetypes"].append(f)
    if inn_places and "ceannai" in arch:
        keep = {"aran", "miodh", "deoch leighis", "capall"}
        k = {kk: v for kk, v in arch["ceannai"].items() if kk not in ("homes",)}
        k.update({"name": INNKEEPER, "title": "Innkeeper", "home": inn_places[0]["name"], "homes": [p["name"] for p in inn_places[1:]],
                  "count": len(inn_places), "work": "square", "color": "#8d5524",
                  "shop": [s for s in arch["ceannai"].get("shop", []) if s["item"] in keep]})
        k["rules"] = [r for r in flee(arch["ceannai"]) if r["then"]["kind"] == "flee"] + [
            {"when": {"kind": "during", "parts": ["night"]}, "then": {"kind": "indoors"}},
            {"when": {"kind": "during", "parts": ["day", "dusk"]}, "then": {"kind": "chat", "lines": [
                "A bed and a hot supper, traveller?", "The road's long; rest your feet a while.", "Mead's fresh today.", "Horses for hire, or for keeps!"]}},
            {"when": {"kind": "sees", "who": "player", "within": 14.0}, "then": {"kind": "chat", "lines": [
                "Come in, come in! You look half dead from the road.", "News from the towns? Sit down and tell it.", "Bread, mead and a draught for your wounds."]}},
        ]
        g["archetypes"].append(k)
        if "villager" in arch:
            v = arch["villager"]
            v["homes"] = (v.get("homes") or []) + [p["name"] for p in inn_places]
            v["count"] = v.get("count", 0) + 2 * len(inn_places)
    return len(farm_places), len(inn_places)


VILLAGE = "sraidbhaile "
VILLAGE_FOLK = {  # bot type: (one in how many villages has one, how many it has)
    "villager": (1, 1), "child": (4, 1), "madra": (3, 1), "farmer": (3, 1),
}
# Bot types whose number is set outright (wildlife and the roads), and the new ones added here.
COUNTS = {"fia": 1200, "sionnach": 350, "damh": 200, "capall fiain": 200, "mac tire": 100, "cearc": 300,
          "ceannai taistil": 12, "fear cairte": 12, "oilithreach": 12, "maor bothair": 10, "dibheirgeach": 30, "meirleach": 12}


def add_villages(g, w):
    """Places and folk for the villages: see the module's doc. `w` is the world file."""
    ours = lambda name: name.startswith(VILLAGE)
    g["places"] = [p for p in g["places"] if not ours(p["name"])]
    g["archetypes"] = [a for a in g["archetypes"] if a["name"] not in ("caora", "torc")]
    base = {}
    for a in g["archetypes"]:
        if a.get("homes"):
            gone = sum(1 for h in a["homes"] if ours(h))
            a["homes"] = [h for h in a["homes"] if not ours(h)]
            if a["name"] in VILLAGE_FOLK:
                # What an earlier run added: its folk per village, taken off again.
                base[a["name"]] = a.get("count", 0) - VILLAGE_FOLK[a["name"]][1] * gone
    arch = {a["name"]: a for a in g["archetypes"]}
    for k in VILLAGE_FOLK:
        if k in arch:
            base.setdefault(k, arch[k].get("count", 0))
    first = ["Baile", "Cill", "Ráth", "Dún", "Áth", "Cluain", "Lios", "Tulach", "Achadh", "Gort", "Doire", "Mainistir"]
    second = ["an Mhuilinn", "na Coille", "Mór", "Beag", "an Droichid", "na hAbhann", "Bhuí", "Glas", "na gCloch",
              "an Chnoic", "Rua", "an Iarainn", "na Móna", "an Fhia", "Locha", "na Sceiche"]
    taken = {p["name"] for p in g["places"]}

    def plain(text):
        import unicodedata
        return "".join(c for c in unicodedata.normalize("NFD", text.lower()) if unicodedata.category(c) != "Mn")

    def unique(name):
        name = name[:32].rstrip()
        n, out = 2, name
        while out in taken:
            tail = f" {n}"
            out, n = name[:32 - len(tail)].rstrip() + tail, n + 1
        taken.add(out)
        return out

    places = []
    for o in w.get("objects", []):
        if o.get("t") != "square":
            continue
        b = o["b"]
        x, y = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        if any(math.hypot(x - p["x"], y - p["y"]) <= p["r"] + 60 for p in g["places"] + places):
            continue
        h = (int(x) * 73856093) ^ (int(y) * 19349663)
        title = f"{first[h % len(first)]} {second[(h // len(first)) % len(second)]}"
        places.append({"name": unique(VILLAGE + plain(title)), "x": round(x, 1), "y": round(y, 1), "r": 70.0, "title": title})
    room = 2048 - len(g["places"])
    places = places[:room]
    g["places"] += places
    names = [p["name"] for p in places]
    for k, (every, n) in VILLAGE_FOLK.items():
        if k not in arch:
            continue
        a = arch[k]
        mine = names[::every]
        a["homes"] = (a.get("homes") or []) + mine
        a["count"] = base[k] + n * len(mine)
    for k, n in COUNTS.items():
        if k in arch:
            arch[k]["count"] = n
    farms = [p["name"] for p in g["places"] if p["name"].startswith(FARM)]
    hen = arch.get("cearc")
    if hen is not None:
        hen["homes"] = [h for h in hen.get("homes", []) if not ours(h)] + names[::2]
    if "aoire" in arch and farms:
        arch["aoire"]["homes"] = farms[1:]
        arch["aoire"]["home"] = farms[0]
        arch["aoire"]["count"] = len(farms) // 3 + 5
    flee = lambda r: [{"when": {"kind": "sees", "who": "any", "within": r}, "then": {"kind": "flee"}}]
    if farms:
        # Sheep have a weapon of sorts (a butt) only so the town's ways (quarrels, gossip, mourning) leave them be.
        g["archetypes"].append({"name": "caora", "title": "Sheep", "color": "#e8e4d8", "count": len(farms) * 4, "behaviour": "wander",
                                "speed_mult": 0.6, "sight": 14.0, "attack": "buille", "attrs": {"health": 30.0}, "home": farms[0],
                                "homes": farms[1:] + names[::5], "rules": flee(8.0), "drops": [], "xp": 2, "level": 1, "skill": "fiach"})
    g["archetypes"].append({"name": "torc", "title": "Wild Boar", "color": "#5a3d2e", "count": 260, "behaviour": "wander",
                            "speed_mult": 1.05, "sight": 30.0, "attack": "buille", "attrs": {"health": 110.0}, "home": None,
                            "rules": [{"when": {"kind": "sees", "who": "player", "within": 9.0}, "then": {"kind": "chase"}}],
                            "drops": [{"item": "aran", "chance_pct": 40.0, "min": 1, "max": 2}], "xp": 22, "level": 4, "skill": "fiach"})
    return len(places)


def big_game(seed_dir, world_path):
    g = load(f"{seed_dir}/game.json")
    ow = load(f"{seed_dir}/world.json")
    size, settled, squares, world = objects_of(world_path)
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
    add_dungeons(g)
    add_country(g, world)

    # New characters, and those whose saved spot was on the old map, come into the world at the capital.
    cap = by_title.get(capital) if capital else None
    if cap:
        g["start_place"] = cap["name"]
    return g


def add_dungeons(g):
    """Instanced dungeons with bosses (the realm's `dungeons`, the bot types' `boss`): each party that walks in gets its
    own copy. They use the game's own spells for their moves, so the content hash is untouched."""
    arch = {a["name"]: a for a in g["archetypes"]}
    places = {p["name"] for p in g["places"]}
    if "crom dubh" in arch and "bru na boinne" in places:
        arch["crom dubh"].update({
            "attrs": {**arch["crom dubh"].get("attrs", {}), "health": 300.0},
            "leash": 80.0,
            "boss": {
                "phases": [
                    {"below": 100, "moves": [
                        {"action": "mallacht", "every_secs": 6, "at": "tank"},
                        {"action": "ceo nimhe", "every_secs": 9, "warn_secs": 2.5, "at": "random", "radius": 7,
                         "say": "Crom Dubh breathes out the grave's air!"}]},
                    {"below": 60, "say": "Crom Dubh calls the dead out of the mound!", "adds": [{"bot": "sluagh", "count": 3}],
                     "moves": [
                        {"action": "mallacht", "every_secs": 6, "at": "tank"},
                        {"action": "ceo nimhe", "every_secs": 8, "warn_secs": 2.2, "at": "random", "radius": 7},
                        {"action": "tintreach", "every_secs": 10, "warn_secs": 2.0, "at": "farthest", "radius": 6}]},
                    {"below": 25, "say": "The dark god rages: the mound fills with poison!",
                     "moves": [
                        {"action": "mallacht", "every_secs": 5, "at": "tank"},
                        {"action": "ceo nimhe", "every_secs": 7, "warn_secs": 2.5, "at": "self", "radius": 10}]},
                ],
                "enrage_secs": 300, "enrage_might": 3,
                "defeat": "Crom Dubh falls, and the mound is quiet.",
            },
        })
    if "dibheirgeach" in arch and "dibheirgigh" in places and "ri na ndibheirgeach" not in arch:
        king = {k: v for k, v in arch["dibheirgeach"].items() if k not in ("names", "surnames", "rules")}
        king.update({
            "name": "ri na ndibheirgeach", "title": "Brigand King", "count": 0, "level": 9, "xp": 300, "color": "#3e2723",
            "ambush": False, "leash": 80.0, "rules": [],
            "attrs": {"health": 300.0},
            "drops": [{"item": "claiomh", "chance_pct": 60.0, "min": 1, "max": 1}, {"item": "pingin airgid", "chance_pct": 100.0, "min": 10, "max": 25}],
            "boss": {
                "phases": [
                    {"below": 100, "moves": [
                        {"action": "liathroid tine", "every_secs": 8, "warn_secs": 2.0, "at": "farthest", "radius": 5,
                         "say": "The Brigand King hurls a firepot!"}]},
                    {"below": 50, "say": "To me, lads!", "adds": [{"bot": "dibheirgeach", "count": 3}],
                     "moves": [
                        {"action": "liathroid tine", "every_secs": 6, "warn_secs": 1.8, "at": "random", "radius": 5}]},
                ],
                "enrage_secs": 240, "enrage_might": 3,
                "defeat": "The Brigand King is down. The camp is yours.",
            },
        })
        g["archetypes"].append(king)
    dungeons = [
        {"name": "Brú na Bóinne", "place": "bru na boinne", "bots": [{"bot": "sluagh", "count": 4}, {"bot": "crom dubh", "count": 1}],
         "xp": 600, "reward": {"item": "fainne oir", "count": 1}},
        {"name": "Campa na nDíbheirgeach", "place": "dibheirgigh", "bots": [{"bot": "dibheirgeach", "count": 5}, {"bot": "ri na ndibheirgeach", "count": 1}],
         "xp": 350, "reward": {"item": "deoch leighis", "count": 3}},
    ]
    have = {d["place"] for d in g.get("dungeons", [])}
    g["dungeons"] = g.get("dungeons", []) + [d for d in dungeons if d["place"] in places and d["place"] not in have]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("what", choices=["recipe", "game", "country", "villages"])
    ap.add_argument("--game", help="country: the game file to edit in place")
    ap.add_argument("--size", type=float, default=65536.0)
    ap.add_argument("--from", dest="src", default=SEED_DIR)
    ap.add_argument("--world")
    a = ap.parse_args()
    if a.what == "recipe":
        json.dump(big_recipe(a.src, a.size), sys.stdout)
    elif a.what == "villages":
        if not (a.world and a.game):
            ap.error("villages needs --world and --game")
        g = load(a.game)
        n = add_villages(g, load(a.world))
        with open(a.game, "w") as f:
            json.dump(g, f, ensure_ascii=False, indent=1)
        print(f"{n} villages; {sum(x.get('count', 0) for x in g['archetypes'])} bots", file=sys.stderr)
    elif a.what == "country":
        if not (a.world and a.game):
            ap.error("country needs --world and --game")
        g = load(a.game)
        farms, inns = add_country(g, load(a.world))
        with open(a.game, "w") as f:
            json.dump(g, f, ensure_ascii=False, indent=1)
        print(f"{farms} farmsteads, {inns} inns", file=sys.stderr)
    else:
        if not a.world:
            ap.error("game needs --world")
        json.dump(big_game(a.src, a.world), sys.stdout, ensure_ascii=False, indent=1)


if __name__ == "__main__":
    main()

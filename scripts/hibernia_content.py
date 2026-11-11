#!/usr/bin/env python3
"""Hibernia's second wave of content: three times the people, gear and work of the first.

    scripts/hibernia_content.py IN.json OUT.json

Reads a game file (the live one, from /api/game, or deploy/hibernia-65536/game.json) and writes it back with:
  - new kinds of folk in the towns and villages (smiths, bakers, herb-wives, druids, storytellers, priests,
    hunters, millers, weavers, tinkers), each with work, talk and, for some, a shop;
  - new beasts and monsters by region (bear, badger, hare, wild bull; the puca, the bean si, the dullahan, the
    gruagach, the kern, the cattle raiders, the will-o'-the-wisp, the cave spider, the far darrig, the
    leprechaun, the Fomorian champion), with loot tables by tier;
  - the existing population about trebled, spread over the villages and farms rather than heaped on Dublin;
  - the gear pass: weapons of every look, armour and trinkets from level 1 to 20, materials, boss trophies,
    food, potions, mounts and dyes, with smithing (gaibhneacht) to make them;
  - quests for the new folk: chains (the smith's, the druid's, the storyteller's tales of the Fianna, the
    herb-wife's, the hunter's), monster hunts by region, and an errand for each of the big towns.

Running it twice changes nothing more: anything already there by name is left as it is (counts are raised to
their targets, never past). Places are the game file's own; a name the file does not have is skipped.
"""
import json
import random
import sys

R = random.Random(65536)


def main():
    src, out = sys.argv[1], sys.argv[2]
    g = json.load(open(src))
    places = {p["name"] for p in g["places"]}
    towns = [n for n in TOWNS if n in places]
    villages = sorted(p["name"] for p in g["places"] if p["name"].startswith("sraidbhaile "))
    farms = sorted(p["name"] for p in g["places"] if p["name"].startswith("feirm "))
    inns = sorted(p["name"] for p in g["places"] if p["name"].startswith("teach osta "))
    mountains = [n for n in MOUNTAINS if n in places]
    lochs = [n for n in LOCHS if n in places]
    holy = [n for n in HOLY if n in places]

    def only(names):
        return [n for n in names if n in places]

    def spread(first, many, k):
        """`first` and up to `k` more of `many` (a fixed pick, so reruns agree)."""
        rest = [n for n in many if n != first]
        pick = R.sample(rest, min(k, len(rest)))
        return first, pick

    items = {i["name"]: i for i in g["items"]}
    for it in ITEMS:
        if it["name"] not in items:
            g["items"].append(it)
            items[it["name"]] = it
    # The old weapons that had a look and no blow behind it.
    for name, eq in FIX_GEAR.items():
        if name in items:
            items[name]["equip"] = eq
            items[name]["consume"] = False
    for name, lvl in GEAR_LEVELS.items():
        if name in items and items[name].get("equip"):
            items[name]["equip"]["level"] = lvl

    arch = {a["name"]: a for a in g["archetypes"]}
    names, surnames = (arch["villager"].get("names", []), arch["villager"].get("surnames", [])) if "villager" in arch else ([], [])

    def add(a):
        if a["name"] in arch:
            return
        g["archetypes"].append(a)
        arch[a["name"]] = a

    # ---- folk
    big = towns
    for f in FOLK:
        home, homes = f.pop("_where")(big, villages, farms, inns, holy, spread)
        a = folk(f, home, homes, names, surnames)
        add(a)

    # ---- beasts and monsters
    for m in BEASTS:
        where = m.pop("_where")
        if where == "wild":
            home, homes = None, []
        else:
            pool = only(where(mountains, lochs, holy, places))
            if not pool:
                continue
            home, homes = pool[0], pool[1:]
        add(beast(m, home, homes))

    # ---- the population, trebled and spread out
    for name, (count, more) in POPULATION.items():
        a = arch.get(name)
        if not a:
            continue
        a["count"] = max(a.get("count", 0), count)
        if more:
            pool = {"villages": villages, "farms": farms, "towns": towns}[more]
            have = set(a.get("homes") or []) | {a.get("home")}
            a["homes"] = (a.get("homes") or []) + [n for n in pool if n not in have]

    # ---- shops for the new folk and the old traders
    for who, stock in SHOPS.items():
        a = arch.get(who)
        if not a:
            continue
        have = {s["item"] for s in a.get("shop", [])}
        a["shop"] = a.get("shop", []) + [{"item": i, "price": p} for i, p in stock if i in items and i not in have]

    # ---- loot tables, and who rolls on them
    have = {t["name"] for t in g.get("loot", []) or []}
    g["loot"] = (g.get("loot") or []) + [t for t in LOOT if t["name"] not in have]
    for who, table in LOOT_OF.items():
        if who in arch and not arch[who].get("loot"):
            arch[who]["loot"] = table
    for who, drops in TROPHIES.items():
        if who in arch:
            got = {d["item"] for d in arch[who].get("drops", [])}
            arch[who]["drops"] = arch[who].get("drops", []) + [d for d in drops if d["item"] not in got]

    # ---- skills and recipes
    have = {s["name"] for s in g.get("skills", [])}
    g["skills"] = g.get("skills", []) + [s for s in SKILLS if s["name"] not in have]
    have = {r["name"] for r in g.get("recipes", [])}
    g["recipes"] = g.get("recipes", []) + [r for r in RECIPES if r["name"] not in have]

    # ---- quests
    have = {q["name"] for q in g["quests"]}
    for q in QUESTS + town_errands(towns, places, g):
        if q["name"] in have:
            continue
        if any(st.get("place") and st["place"] not in places for st in q["steps"]):
            continue
        g["quests"].append(q)
        have.add(q["name"])

    # ---- achievements
    have = {a["name"] for a in g.get("achievements", [])}
    g["achievements"] = g.get("achievements", []) + [a for a in ACHIEVEMENTS if a["name"] not in have and a["deed"].split(":")[-1] in arch | {"kills": 1, "quests": 1, "crafts": 1}]

    json.dump(g, open(out, "w"), ensure_ascii=False, indent=1)
    print(f"{len(g['archetypes'])} bot types, {sum(a.get('count', 0) for a in g['archetypes'])} bots, {len(g['items'])} items, {len(g['quests'])} quests, {len(g['recipes'])} recipes")


# ------------------------------------------------------------------------------------------------ places

TOWNS = ["dublin", "waterford", "cork", "limerick", "galway", "wexford", "armagh", "derry", "kilkenny", "drogheda", "sligo", "athlone",
         "dundalk", "kells", "clonmel", "tralee", "downpatrick", "ennis", "roscommon", "kildare", "carlow", "monaghan", "donegal",
         "westport", "lismore", "coleraine", "enniskillen", "belfast", "mullingar", "thurles", "killarney", "bantry", "youghal",
         "kinsale", "ardee", "listowel"]
TOWN_TITLES = {"dublin": "Áth Cliath", "waterford": "Port Láirge", "cork": "Corcaigh", "limerick": "Luimneach", "galway": "Gaillimh",
               "wexford": "Loch Garman", "armagh": "Ard Mhacha", "derry": "Doire", "kilkenny": "Cill Chainnigh", "drogheda": "Droichead Átha",
               "sligo": "Sligeach", "athlone": "Baile Átha Luain", "dundalk": "Dún Dealgan", "kells": "Ceanannas", "clonmel": "Cluain Meala",
               "tralee": "Trá Lí", "downpatrick": "Dún Pádraig", "ennis": "Inis", "roscommon": "Ros Comáin", "kildare": "Cill Dara",
               "carlow": "Ceatharlach", "monaghan": "Muineachán", "donegal": "Dún na nGall", "westport": "Cathair na Mart",
               "lismore": "Lios Mór", "coleraine": "Cúil Raithin", "enniskillen": "Inis Ceithleann", "belfast": "Béal Feirste",
               "mullingar": "An Muileann gCearr", "thurles": "Durlas", "killarney": "Cill Airne", "bantry": "Beanntraí",
               "youghal": "Eochaill", "kinsale": "Cionn tSáile", "ardee": "Baile Átha Fhirdia", "listowel": "Lios Tuathail"}
MOUNTAINS = ["beanna boirche", "sleibhte chill mhantain", "na cruacha dubha", "na gaibhlte", "sleibhte an chomaraigh", "na beanna beola",
             "cruach phadraig", "binn ghulbain", "earagail", "sliabh speirin", "sliabh bladhma", "na staighri dubha", "cnoc mhaoldomhnaigh",
             "sliabh gcuillinn", "binn chuilceach", "sliabh mis", "cnoc breanainn"]
LOCHS = ["loch neathach", "loch coirib", "loch measca", "loch deirgeirt", "loch ri", "loch ailin", "loch eirne", "loch gile", "loch lein"]
HOLY = ["glendalough", "clonmacnoise", "cashel", "kells", "armagh", "downpatrick", "lismore"]

# ------------------------------------------------------------------------------------------------ talk

FLEE_FROM = ["lochlannach", "tine", "sluagh mairseala", "meirleach", "dubhlachan", "puca"]


def folk(f, home, homes, names, surnames):
    """A townsperson: flees what townsfolk flee, goes in at night and in the rain, talks of their day."""
    rules = [{"when": {"kind": "sees_kind", "bot": b, "within": 30.0}, "then": {"kind": "flee"}} for b in FLEE_FROM if b != f["name"]]
    rules += [
        {"when": {"kind": "weather", "names": ["baisteach"]}, "then": {"kind": "indoors"}},
        {"when": {"kind": "during", "parts": ["night"]}, "then": {"kind": "indoors"}},
        {"when": {"kind": "during", "parts": ["dawn"]}, "then": {"kind": "chat", "lines": f.pop("dawn")}},
        {"when": {"kind": "during", "parts": ["day"]}, "then": {"kind": "chat", "lines": f.pop("day")}},
        {"when": {"kind": "sees", "who": "player", "within": 14.0}, "then": {"kind": "chat", "lines": f.pop("hello")}},
    ]
    a = {"behaviour": "wander", "speed_mult": 0.6, "sight": 30.0, "attack": None, "attrs": {}, "home": home, "rules": rules,
         "drops": [{"item": "pingin airgid", "chance_pct": 40.0, "min": 1, "max": 2}], "names": names, "surnames": surnames}
    a.update(f)
    if homes:
        a["homes"] = homes
    return a


def beast(m, home, homes):
    lvl = m["level"]
    # Health tops out at the attribute's max (300); past that a level makes it hit harder.
    a = {"behaviour": "chase", "speed_mult": 1.0, "sight": 40.0, "attack": "buille", "attrs": {"health": float(min(300, m.pop("hp", 20 * lvl)))},
         "home": home, "rules": [], "drops": [], "xp": m.pop("xp", 6 * lvl + 4)}
    a.update(m)
    if homes:
        a["homes"] = homes
    if home is None:
        a.pop("leash", None)
    return a


def towns_and(k):
    def where(towns, villages, farms, inns, holy, spread):
        home, more = spread(towns[0], towns, len(towns))
        return home, more + R.sample(villages, min(k, len(villages)))
    return where


def holy_places(towns, villages, farms, inns, holy, spread):
    return holy[0], holy[1:] + R.sample(villages, min(60, len(villages)))


def farmland(towns, villages, farms, inns, holy, spread):
    return farms[0], farms[1:] + R.sample(villages, min(120, len(villages)))


def roads(towns, villages, farms, inns, holy, spread):
    return towns[0], towns[1:] + inns


FOLK = [
    dict(name="gabha", title="Smith", color="#5b4a3f", count=180, work="square", _where=towns_and(140),
         dawn=["The forge won't light itself.", "Iron's cold this morning."],
         day=["Hold still, I'm nearly done.", "Mind the sparks!", "Good iron is hard to come by.", "Another horse, another shoe."],
         hello=["Need a blade mended? Or a new one?", "Bring me iron, I'll make you a sword.", "Troll hide, now, that makes a coat."]),
    dict(name="baicear", title="Baker", color="#c9a46b", count=160, work="square", _where=towns_and(124),
         dawn=["Up before the cock to knead the dough.", "First loaves are out!"],
         day=["Bread! Fresh bread!", "Honey cakes for the children.", "Flour's dear this year."],
         hello=["A loaf for the road?", "You look thin. Eat something.", "The miller's late with the flour again."]),
    dict(name="bean feasa", title="Herb-wife", color="#6b8a4a", count=120, work="fields", _where=towns_and(84),
         dawn=["The dew's best for picking.", "Comfrey, yarrow, a little self-heal."],
         day=["There's a cure for most things, if you know where to look.", "Don't touch that, it's not for eating."],
         hello=["You've a cough on you. Here, take this.", "I'll pay for good herbs.", "The wisps in the bog lead fools astray."]),
    dict(name="drui", title="Druid", color="#3d5a3a", count=40, speed_mult=0.45, _where=holy_places,
         dawn=["The sun comes up over the stones.", "The birds speak plainly this morning."],
         day=["Every oak remembers.", "Listen. The land is talking.", "The old ways are not gone, only quiet."],
         hello=["The Lia Fáil stirs, they say.", "Bring me holly and I'll cut you a staff.", "The Morrígan watches the crows."]),
    dict(name="seanchai", title="Storyteller", color="#7a5a8a", count=60, speed_mult=0.45, work="square", _where=towns_and(24),
         dawn=["A story for the morning? No, they're for the night."],
         day=["Did I tell you of Fionn and the salmon?", "There was a giant once, under Sliabh na mBan...", "Every road has a tale."],
         hello=["Sit, sit. Have you heard of the Fianna?", "A tale for a penny, a true one for two.", "Oisín came back from Tír na nÓg, you know."]),
    dict(name="sagart", title="Priest", color="#2a2a36", count=60, speed_mult=0.45, _where=holy_places,
         dawn=["Matins rung. God keep the day."],
         day=["Give alms, and it will come back to you.", "Pray for the dead in the mound."],
         hello=["God bless you, child.", "The dead walk at Brú na Bóinne. Pray for them.", "Light a candle for your mother."]),
    dict(name="sealgaire", title="Hunter", color="#5a6a3a", count=90, speed_mult=0.8, attack="buille", _where=farmland,
         dawn=["Tracks in the frost. Boar, I'd say.", "Out early, the deer are."],
         day=["Quiet. You'll scare them off.", "There's a bear about, I've seen the marks."],
         hello=["Bear pelts fetch a price.", "The bull in the low field has killed two men.", "Leave the hares to me."]),
    dict(name="muilleoir", title="Miller", color="#b8ab90", count=70, work="water", _where=farmland,
         dawn=["Wheel's turning. Good."],
         day=["The stream's low, the stones are slow.", "Flour, flour, nothing but flour."],
         hello=["Wheat to grind?", "The baker owes me for three sacks.", "The far darrig plays tricks on my wheel at night."]),
    dict(name="figheadoir", title="Weaver", color="#8a4a5a", count=80, work="square", _where=towns_and(56),
         dawn=["The loom's waiting."],
         day=["Warp and weft, warp and weft.", "Madder for red, woad for blue."],
         hello=["Wool for a cloak?", "A good tunic keeps out the worst of it.", "Saffron's dear, but it's worth it."]),
    dict(name="tinceir", title="Tinker", color="#8a6a3a", count=60, speed_mult=0.8, _where=roads,
         dawn=["On the road again."],
         day=["Pots mended! Knives sharpened!", "The road goes on forever."],
         hello=["Anything to mend?", "I've been everywhere, me.", "Want to know a secret? It'll cost you."]),
]

BEASTS = [
    dict(name="bear", title="Bear", color="#4a3628", level=6, hp=220, speed_mult=0.9, sight=30.0, behaviour="wander", skill="fiach",
         rules=[{"when": {"kind": "sees", "who": "player", "within": 12.0}, "then": {"kind": "chase"}}], count=160, leash=60.0,
         drops=[{"item": "craiceann beir", "chance_pct": 70.0, "min": 1, "max": 1}], _where=lambda m, l, h, p: ["coill na mac tire"] + m),
    dict(name="broc", title="Badger", color="#6a6a6a", level=1, hp=30, behaviour="wander", speed_mult=0.8, sight=20.0, attack=None, count=500, skill="fiach",
         rules=[{"when": {"kind": "sees", "who": "any", "within": 12.0}, "then": {"kind": "flee"}}],
         drops=[{"item": "craiceann broic", "chance_pct": 50.0, "min": 1, "max": 1}], _where="wild"),
    dict(name="giorria", title="Hare", color="#a58a6a", level=1, hp=15, behaviour="wander", speed_mult=1.4, sight=30.0, attack=None, count=900, skill="fiach",
         rules=[{"when": {"kind": "sees", "who": "any", "within": 20.0}, "then": {"kind": "flee"}}],
         drops=[{"item": "feoil", "chance_pct": 60.0, "min": 1, "max": 1}], _where="wild"),
    dict(name="tarbh fiain", title="Wild Bull", color="#3a2a22", level=5, hp=200, behaviour="wander", speed_mult=1.1, sight=20.0, count=200, skill="fiach",
         rules=[{"when": {"kind": "sees", "who": "player", "within": 10.0}, "then": {"kind": "chase"}}],
         drops=[{"item": "adharc tairbh", "chance_pct": 60.0, "min": 1, "max": 2}, {"item": "feoil", "chance_pct": 80.0, "min": 1, "max": 3}], _where="wild"),
    dict(name="puca", title="Púca", color="#1e1e28", level=7, hp=150, speed_mult=1.3, sight=45.0, count=120, ambush=True, leash=80.0,
         drops=[{"item": "pingin airgid", "chance_pct": 50.0, "min": 1, "max": 4}], _where=lambda m, l, h, p: [n for n in m if n.startswith(("cnoc", "sliabh", "binn"))]),
    dict(name="bean si", title="Bean Sí", color="#d8dde8", level=12, hp=280, speed_mult=0.9, sight=50.0, behaviour="idle", count=24, leash=40.0,
         rules=[{"when": {"kind": "sees", "who": "player", "within": 22.0}, "then": {"kind": "chase"}}],
         drops=[{"item": "deoch draiochta", "chance_pct": 40.0, "min": 1, "max": 1}], _where=lambda m, l, h, p: ["bru na boinne", "rathcroghan", "dun ailinne"] + l[:4]),
    dict(name="dubhlachan", title="Dullahan", color="#151515", level=14, hp=360, speed_mult=1.5, sight=60.0, count=16, ambush=True, leash=120.0,
         drops=[{"item": "pingin airgid", "chance_pct": 100.0, "min": 5, "max": 12}], _where=lambda m, l, h, p: ["bealach na gcreach", "na staighri dubha", "sliabh bladhma", "na gaibhlte"]),
    dict(name="gruagach", title="Gruagach", color="#7a6a4a", level=13, hp=420, speed_mult=0.7, sight=30.0, behaviour="idle", count=30, leash=60.0,
         rules=[{"when": {"kind": "sees", "who": "player", "within": 18.0}, "then": {"kind": "chase"}}],
         drops=[{"item": "craiceann trolla", "chance_pct": 60.0, "min": 1, "max": 2}], _where=lambda m, l, h, p: ["na cruacha dubha", "earagail", "binn ghulbain", "na beanna beola", "cruach phadraig"]),
    dict(name="ceithearnach", title="Kern", color="#5a4a2a", level=6, hp=140, speed_mult=1.05, sight=40.0, count=140, leash=70.0,
         drops=[{"item": "pingin airgid", "chance_pct": 60.0, "min": 1, "max": 4}, {"item": "mein iarainn", "chance_pct": 20.0, "min": 1, "max": 2}],
         names=["Art", "Tadhg", "Murchadh", "Lorcán", "Fiach", "Turlough"], surnames=["Ó Broin", "Ó Tuathail", "Mac Murchadha", "Ó Mórdha"],
         _where=lambda m, l, h, p: ["sleibhte chill mhantain", "na gaibhlte", "sliabh bladhma", "sleibhte an chomaraigh", "sliabh gcuillinn"]),
    dict(name="creachadoir", title="Cattle Raider", color="#6a3a2a", level=9, hp=190, speed_mult=1.1, sight=45.0, count=80, leash=80.0,
         drops=[{"item": "pingin airgid", "chance_pct": 70.0, "min": 2, "max": 6}, {"item": "adharc tairbh", "chance_pct": 30.0, "min": 1, "max": 1}],
         _where=lambda m, l, h, p: ["binn chuilceach", "sliabh mis", "cnoc breanainn", "beanna boirche", "na staighri dubha"]),
    dict(name="solas si", title="Will-o'-the-wisp", color="#bfe8ff", level=4, hp=60, speed_mult=1.2, sight=35.0, count=180, ambush=True, leash=60.0,
         drops=[{"item": "luibheanna", "chance_pct": 50.0, "min": 1, "max": 2}], _where=lambda m, l, h, p: l),
    dict(name="damhan alla mor", title="Cave Spider", color="#2a2a2a", level=7, hp=150, speed_mult=1.15, sight=28.0, behaviour="idle", count=90, leash=50.0,
         rules=[{"when": {"kind": "sees", "who": "player", "within": 16.0}, "then": {"kind": "chase"}}],
         drops=[{"item": "sioda damhain", "chance_pct": 60.0, "min": 1, "max": 2}], _where=lambda m, l, h, p: ["coill na mac tire", "na cruacha dubha", "sliabh speirin", "na staighri dubha"]),
    dict(name="fear dearg", title="Far Darrig", color="#a8322d", level=5, hp=90, speed_mult=1.2, sight=30.0, behaviour="wander", attack=None, count=60,
         rules=[{"when": {"kind": "sees", "who": "player", "within": 10.0}, "then": {"kind": "chat", "lines": ["Hee hee! Catch me if you can!", "A riddle, a riddle! What has roots nobody sees?", "Your purse is lighter than you think."]}},
                {"when": {"kind": "sees", "who": "player", "within": 6.0}, "then": {"kind": "flee"}}],
         drops=[{"item": "pingin airgid", "chance_pct": 100.0, "min": 3, "max": 8}], _where=lambda m, l, h, p: m[:8]),
    dict(name="leipreachan", title="Leprechaun", color="#2f6b3a", level=3, hp=40, speed_mult=1.5, sight=35.0, behaviour="wander", attack=None, count=50,
         rules=[{"when": {"kind": "sees", "who": "player", "within": 16.0}, "then": {"kind": "flee"}}],
         drops=[{"item": "pingin airgid", "chance_pct": 100.0, "min": 8, "max": 20}, {"item": "bonn oir", "chance_pct": 25.0, "min": 1, "max": 1}],
         _where=lambda m, l, h, p: ["coill na mac tire"] + l[:3] + m[:4]),
    dict(name="curadh fomhorach", title="Fomorian Champion", color="#2f4a22", level=16, hp=520, speed_mult=1.0, sight=50.0, count=12, leash=100.0,
         drops=[{"item": "cnamh fomhorach", "chance_pct": 80.0, "min": 1, "max": 2}], _where=lambda m, l, h, p: ["an tuaisceart 1", "an tuaisceart 2", "an tuaisceart 3", "an tuaisceart 4", "an tuaisceart 5"]),
]

# Counts to raise to; villages/farms/towns: also lives in every one of those.
POPULATION = {
    "villager": (3400, "villages"), "farmer": (1150, "villages"), "child": (800, "villages"), "madra": (1000, "villages"),
    "watchman": (90, "towns"), "fisher": (100, None), "gadai": (30, "towns"), "feirmeoir": (290, None), "aoire": (110, None),
    "ostoir": (34, None), "cearc": (900, None), "caora": (1100, None), "fia": (2600, None), "sionnach": (800, None),
    "torc": (600, None), "damh": (480, None), "capall fiain": (420, None), "mac tire": (300, None), "loscann": (220, None),
    "beacanan": (180, None), "troll cnoic": (90, None), "fomhorach": (150, None), "troll sneachta": (100, None),
    "mac tire ban": (100, None), "mac tire ban thiar": (80, None), "dibheirgeach": (60, None), "meirleach": (36, None),
    "maor bothair": (30, None), "ceannai taistil": (36, None), "fear cairte": (36, None), "oilithreach": (36, None),
    "tuathanach": (40, "towns"), "ceannai": (20, "towns"), "laoch": (12, "towns"), "iascaire": (16, None), "manach": (16, None),
    "garda": (24, None), "peist locha": (12, None),
}

# ------------------------------------------------------------------------------------------------ items


def weapon(name, title, look, lvl, might, value, rarity="common", colour="#a0a4ac"):
    it = {"name": name, "title": title, "color": colour, "stack_max": 1, "uses": [], "consume": False, "value": value,
          "equip": {"slot": "weapon", "might": might, "level": lvl}, "look": look}
    if rarity != "common":
        it["rarity"] = rarity
    return it


def armour(name, title, lvl, tough, value, rarity="common", colour="#7d6a52", might=0.0):
    eq = {"slot": "armour", "toughness": tough, "level": lvl}
    if might:
        eq["might"] = might
    it = {"name": name, "title": title, "color": colour, "stack_max": 1, "uses": [], "consume": False, "value": value, "equip": eq}
    if rarity != "common":
        it["rarity"] = rarity
    return it


def trinket(name, title, lvl, might, tough, value, rarity="common", colour="#c9a227"):
    eq = {"slot": "trinket", "level": lvl}
    if might:
        eq["might"] = might
    if tough:
        eq["toughness"] = tough
    it = {"name": name, "title": title, "color": colour, "stack_max": 1, "uses": [], "consume": False, "value": value, "equip": eq}
    if rarity != "common":
        it["rarity"] = rarity
    return it


def stuff(name, title, colour, value, stack=20, rarity="common"):
    it = {"name": name, "title": title, "color": colour, "stack_max": stack, "uses": [], "consume": False, "value": value}
    if rarity != "common":
        it["rarity"] = rarity
    return it


def food(name, title, colour, value, health=0.0, energy=0.0, mana=0.0, stack=10):
    uses = [{"attr": a, "delta": d} for a, d in (("health", health), ("energy", energy), ("mana", mana)) if d]
    return {"name": name, "title": title, "color": colour, "stack_max": stack, "uses": uses, "consume": True, "value": value}


ITEMS = [
    # Swords
    weapon("claiomh cre-umha", "Bronze Sword", "sword", 1, 0.06, 4, colour="#b08d57"),
    weapon("claiomh iarainn", "Iron Sword", "sword", 6, 0.24, 18),
    weapon("claiomh cruach", "Steel Sword", "sword", 10, 0.34, 40, "uncommon", "#d0d4dc"),
    weapon("claiomh lochlannach", "Norse Blade", "sword", 14, 0.44, 80, "rare", "#9fb0c0"),
    weapon("fragarach", "Fragarach, the Answerer", "sword", 20, 0.72, 400, "legendary", "#e8f0ff"),
    # Greatswords
    weapon("claiomh mor", "Greatsword", "greatsword", 8, 0.30, 30),
    weapon("claiomh mor na laoch", "Champion's Greatsword", "greatsword", 13, 0.42, 90, "rare", "#c0c8d8"),
    weapon("caladbolg", "Caladbolg", "greatsword", 20, 0.75, 420, "legendary", "#ffe6a8"),
    # Axes
    weapon("tua bheag", "Hatchet", "axe", 1, 0.07, 4, colour="#8a7a6a"),
    weapon("tua iarainn", "Iron Axe", "axe", 5, 0.20, 16),
    weapon("tua lochlannach", "Dane Axe", "axe", 12, 0.40, 75, "rare", "#9aa0a8"),
    # Greataxes
    weapon("tua mhor huscarla", "Huscarl's Greataxe", "greataxe", 17, 0.58, 160, "epic", "#b0b8c8"),
    # Staves
    weapon("lorg dharach", "Oak Staff", "staff", 7, 0.26, 22, colour="#7a5a3a"),
    weapon("lorg cuilinn", "Holly Staff", "staff", 12, 0.40, 70, "rare", "#3f6a3a"),
    weapon("lorg na ndrui", "Archdruid's Staff", "staff", 18, 0.62, 180, "epic", "#5a8a4a"),
    # Wands
    weapon("slat chuill", "Hazel Wand", "wand", 3, 0.12, 8, colour="#9a7a4a"),
    weapon("slat iubhair", "Yew Wand", "wand", 9, 0.30, 34, "uncommon", "#6a3a2a"),
    weapon("slat na si", "Wand of the Sídhe", "wand", 16, 0.55, 150, "epic", "#bfe8ff"),
    # Knives
    weapon("scian chnamh", "Bone Knife", "knife", 4, 0.14, 8, colour="#e0d8c0"),
    weapon("miodog", "Dirk", "knife", 9, 0.28, 30, "uncommon"),
    weapon("fiacail fhomhorach", "Fomorian Fang", "knife", 15, 0.50, 110, "rare", "#d8e0b0"),
    # Crossbows
    weapon("crosbhogha trom", "Heavy Crossbow", "crossbow", 11, 0.38, 60, "uncommon", "#5a4a3a"),
    weapon("crosbhogha an fhiaga", "Huntsman's Crossbow", "crossbow", 16, 0.54, 150, "epic", "#6a5a2a"),
    # Armour
    armour("ionar olla", "Wool Tunic", 1, 0.05, 3, colour="#b8a88a"),
    armour("seaicead leathair", "Leather Jerkin", 3, 0.10, 8, colour="#7a5232"),
    armour("cota fionnaidh", "Fur Coat", 7, 0.22, 24, colour="#8a7560"),
    armour("luireach throill", "Troll-hide Coat", 10, 0.30, 50, "rare", "#9a8466"),
    armour("luireach lann", "Wyrm-scale Hauberk", 14, 0.40, 90, "rare", "#4a7a6a"),
    armour("luireach huscarla", "Huscarl's Hauberk", 17, 0.50, 170, "epic", "#8a96a8"),
    armour("brat na si", "Mantle of the Sídhe", 20, 0.60, 400, "legendary", "#d8dde8", might=0.08),
    # Trinkets
    trinket("dealg chre-umha", "Bronze Brooch", 1, 0.03, 0.02, 4, colour="#b08d57"),
    trinket("torc cre-umha", "Bronze Torc", 4, 0.06, 0.03, 10, colour="#b08d57"),
    trinket("torc oir", "Gold Torc", 10, 0.12, 0.06, 45, "uncommon"),
    trinket("cloch nathrach", "Adder Stone", 12, 0.0, 0.14, 60, "rare", "#6a8a9a"),
    trinket("briocht fiaigh", "Raven Charm", 13, 0.16, 0.0, 70, "rare", "#2a2a36"),
    trinket("lunula oir", "Gold Lunula", 15, 0.14, 0.10, 120, "epic", "#f1c40f"),
    trinket("slis lia fail", "Shard of the Lia Fáil", 20, 0.22, 0.18, 400, "legendary", "#e8e0d0"),
    # Materials
    stuff("mein iarainn", "Iron Ore", "#6a5a52", 1),
    stuff("barra iarainn", "Iron Bar", "#8a8f96", 3),
    stuff("gual", "Charcoal", "#2a2a2a", 1),
    stuff("adhmad", "Timber", "#8a5a34", 1),
    stuff("cuileann", "Holly Branch", "#3f6a3a", 2, 10),
    stuff("olann", "Wool", "#efe8d8", 1),
    stuff("craiceann broic", "Badger Pelt", "#7a7a7a", 2, 10),
    stuff("craiceann beir", "Bear Pelt", "#4a3628", 6, 10, "uncommon"),
    stuff("craiceann trolla", "Troll Hide", "#9a8466", 10, 10, "uncommon"),
    stuff("lann peiste", "Wyrm Scale", "#4a7a6a", 15, 10, "rare"),
    stuff("cnamh fomhorach", "Fomorian Bone", "#d8e0b0", 12, 10, "rare"),
    stuff("cleite fiaigh", "Raven Feather", "#2a2a36", 4, 20, "uncommon"),
    stuff("adharc tairbh", "Bull's Horn", "#d8c8a8", 3, 10),
    stuff("sioda damhain", "Spider Silk", "#e8e8f0", 4, 20, "uncommon"),
    stuff("luibheanna", "Herbs", "#6b8a4a", 1),
    stuff("bonn oir", "Gold Piece", "#f1c40f", 10, 20, "uncommon"),
    # Trophies
    stuff("croi chrom dubh", "Heart of Crom Dubh", "#3a2a4a", 50, 1, "epic"),
    stuff("coroin an ri", "Brigand King's Crown", "#c9a227", 40, 1, "rare"),
    stuff("cleite na morrigan", "Feather of the Morrígan", "#1a1a24", 60, 1, "epic"),
    stuff("fainne an iarla", "Jarl's Arm-ring", "#d0b060", 60, 1, "epic"),
    stuff("fiacail peiste", "Wyrm's Tooth", "#efe8d0", 50, 1, "epic"),
    # Food
    food("feoil", "Raw Meat", "#a8423a", 1, energy=5.0, stack=20),
    food("feoil rosta", "Roast Meat", "#8a4a2a", 2, health=18.0, energy=25.0),
    food("bradan", "Salmon", "#e08a6a", 3, health=20.0, energy=15.0),
    food("cais", "Cheese", "#f0d070", 2, health=10.0, energy=20.0),
    food("im", "Butter", "#f5e08a", 1, energy=15.0),
    food("bainne", "Milk", "#f4f4ee", 1, health=4.0, energy=10.0),
    food("ubh", "Egg", "#f0e8d8", 1, energy=8.0, stack=20),
    food("mil", "Honey", "#e8a23a", 3, health=8.0, energy=20.0),
    food("smeara", "Blackberries", "#3a1a3a", 1, health=5.0, energy=5.0, stack=20),
    food("cno", "Hazelnuts", "#a0703a", 1, energy=10.0, mana=5.0, stack=20),
    food("stobhach", "Stew", "#8a6a3a", 4, health=30.0, energy=40.0),
    food("caca meala", "Honey Cake", "#d8a050", 4, health=15.0, energy=35.0),
    food("beoir", "Ale", "#c08a3a", 2, health=6.0, energy=18.0),
    # Potions
    {"name": "deoch leighis mhor", "title": "Greater Healing Draught", "color": "#e0302a", "stack_max": 5, "uses": [{"attr": "health", "delta": 90.0}], "consume": True, "value": 8, "rarity": "uncommon"},
    {"name": "deoch chosanta", "title": "Warding Draught", "color": "#6a7a9a", "stack_max": 5, "uses": [], "consume": True, "value": 8, "rarity": "rare", "potion": {"secs": 90.0, "toughness": 0.5}},
    {"name": "deoch laoich", "title": "Hero's Draught", "color": "#e06a1a", "stack_max": 5, "uses": [], "consume": True, "value": 10, "rarity": "rare", "potion": {"secs": 90.0, "might": 0.45}},
    # Mounts
    {"name": "capaillin", "title": "Pony", "color": "#8a6a4a", "stack_max": 1, "uses": [], "consume": False, "value": 10, "mount": 0.5},
    {"name": "capall cogaidh", "title": "Warhorse", "color": "#3a2a22", "stack_max": 1, "uses": [], "consume": False, "value": 80, "mount": 1.1, "rarity": "rare"},
    # Dyes
    {"name": "dath dubh", "title": "Black Dye", "color": "#22201e", "stack_max": 10, "uses": [], "consume": True, "value": 6, "dye": "#22201e"},
    {"name": "corcra", "title": "Royal Purple", "color": "#5b2a6e", "stack_max": 10, "uses": [], "consume": True, "value": 20, "dye": "#5b2a6e", "rarity": "rare"},
]

FIX_GEAR = {
    "lorg": {"slot": "weapon", "might": 0.10, "level": 2},
    "scian": {"slot": "weapon", "might": 0.08, "level": 1},
    "crosbhogha": {"slot": "weapon", "might": 0.20, "level": 5},
}
GEAR_LEVELS = {"claiomh": 3, "tua chatha": 6, "luireach": 5, "clogad lochlannach": 4, "fainne oir": 8}

SKILLS = [{"name": "gaibhneacht", "title": "Smithing", "max": 100}]


def rec(name, needs, makes=None, count=1, skill="gaibhneacht", level=0):
    r = {"name": name, "needs": [{"item": i, "count": c} for i, c in needs], "makes": makes or name, "count": count, "skill": skill}
    if level:
        r["level"] = level
    return r


RECIPES = [
    rec("barra iarainn", [("mein iarainn", 2), ("gual", 1)]),
    rec("gual", [("adhmad", 2)], count=2),
    rec("claiomh iarainn", [("barra iarainn", 3), ("adhmad", 1)], level=5),
    rec("tua iarainn", [("barra iarainn", 2), ("adhmad", 2)], level=5),
    rec("claiomh cruach", [("barra iarainn", 5), ("gual", 3)], level=20),
    rec("claiomh mor", [("barra iarainn", 6), ("adhmad", 1)], level=15),
    rec("miodog", [("barra iarainn", 2), ("adharc tairbh", 1)], level=12),
    rec("seaicead leathair", [("craiceann broic", 3)], level=1),
    rec("cota fionnaidh", [("craiceann beir", 2), ("craiceann mic tire", 2)], level=10),
    rec("luireach throill", [("craiceann trolla", 3), ("barra iarainn", 2)], level=25),
    rec("luireach lann", [("lann peiste", 4), ("barra iarainn", 3)], level=40),
    rec("fiacail fhomhorach", [("cnamh fomhorach", 2), ("barra iarainn", 1)], level=35),
    rec("lorg dharach", [("adhmad", 4)], level=8),
    rec("lorg cuilinn", [("cuileann", 4), ("cleite fiaigh", 2)], level=25),
    rec("crosbhogha trom", [("adhmad", 3), ("barra iarainn", 2), ("sioda damhain", 2)], level=22),
    rec("briocht fiaigh", [("cleite fiaigh", 6), ("bonn oir", 1)], level=30),
    rec("torc oir", [("bonn oir", 4)], level=20),
    rec("luireach huscarla", [("fainne an iarla", 1), ("barra iarainn", 6)], level=55),
    rec("tua mhor huscarla", [("fainne an iarla", 1), ("barra iarainn", 5), ("adhmad", 2)], level=55),
    rec("lorg na ndrui", [("cleite na morrigan", 1), ("cuileann", 4)], level=60),
    rec("slat na si", [("cleite na morrigan", 1), ("sioda damhain", 4)], level=60),
    rec("crosbhogha an fhiaga", [("fiacail peiste", 1), ("adhmad", 3), ("sioda damhain", 3)], level=60),
    rec("lunula oir", [("coroin an ri", 1), ("bonn oir", 4)], level=50),
    rec("fragarach", [("croi chrom dubh", 1), ("fiacail peiste", 1), ("barra iarainn", 8)], level=80),
    rec("caladbolg", [("fainne an iarla", 1), ("cleite na morrigan", 1), ("barra iarainn", 10)], level=85),
    rec("slis lia fail", [("croi chrom dubh", 1), ("cleite na morrigan", 1), ("fiacail peiste", 1)], level=90),
    rec("brat na si", [("cleite na morrigan", 1), ("sioda damhain", 10), ("lann peiste", 4)], level=90),
    rec("feoil rosta", [("feoil", 2)], skill="cocaireacht"),
    rec("stobhach", [("feoil", 2), ("meacan", 2), ("cabaiste", 1)], skill="cocaireacht", level=15),
    rec("caca meala", [("cruithneacht", 2), ("mil", 1), ("ubh", 1)], skill="cocaireacht", level=20),
    rec("cais", [("bainne", 3)], skill="cocaireacht", level=5),
    rec("deoch leighis", [("luibheanna", 3)], skill="ailceimic"),
    rec("deoch leighis mhor", [("luibheanna", 4), ("mil", 1)], skill="ailceimic", level=25),
    rec("deoch chosanta", [("luibheanna", 3), ("craiceann trolla", 1)], skill="ailceimic", level=35),
    rec("deoch laoich", [("luibheanna", 3), ("adharc tairbh", 1), ("miodh", 1)], skill="ailceimic", level=40),
]

SHOPS = {
    "laoch": [("claiomh cre-umha", 8), ("tua bheag", 8), ("claiomh iarainn", 45), ("tua iarainn", 40), ("ionar olla", 6), ("seaicead leathair", 18)],
    "gabha": [("claiomh iarainn", 42), ("tua iarainn", 36), ("claiomh mor", 70), ("scian chnamh", 18), ("miodog", 60), ("seaicead leathair", 16),
              ("luireach", 40), ("cota fionnaidh", 55), ("barra iarainn", 6), ("mein iarainn", 2), ("gual", 2), ("crosbhogha", 40)],
    "baicear": [("aran", 2), ("cais", 4), ("caca meala", 7), ("im", 2), ("ubh", 1), ("beoir", 3)],
    "bean feasa": [("deoch leighis", 6), ("deoch leighis mhor", 16), ("luibheanna", 2), ("deoch athnuachana", 8), ("smeara", 1)],
    "drui": [("lorg", 30), ("slat chuill", 18), ("lorg dharach", 50), ("slat iubhair", 75), ("deoch draiochta", 9), ("cno", 2)],
    "figheadoir": [("ionar olla", 6), ("olann", 2), ("glaisin", 8), ("madar", 8), ("croch", 8), ("dath dubh", 10), ("corcra", 40)],
    "tinceir": [("scian", 15), ("dealg chre-umha", 8), ("torc cre-umha", 20), ("coinnle", 4), ("bairille", 6), ("capaillin", 30)],
    "muilleoir": [("cruithneacht", 2), ("coirce", 2), ("siol cruithneachta", 1)],
    "sealgaire": [("crosbhogha", 38), ("feoil rosta", 4), ("craiceann broic", 4), ("scian chnamh", 16)],
    "ceannai": [("dealg chre-umha", 8), ("torc cre-umha", 20), ("torc oir", 95), ("mil", 5), ("bradan", 5)],
    "ostoir": [("stobhach", 6), ("beoir", 3), ("feoil rosta", 4), ("capaillin", 30)],
}

LOOT = [
    {"name": "creach iseal", "rolls": 1, "nothing": 70, "entries": [
        {"item": "claiomh cre-umha", "weight": 6}, {"item": "tua bheag", "weight": 6}, {"item": "scian chnamh", "weight": 4}, {"item": "slat chuill", "weight": 4},
        {"item": "ionar olla", "weight": 6}, {"item": "seaicead leathair", "weight": 4}, {"item": "dealg chre-umha", "weight": 4}, {"item": "torc cre-umha", "weight": 2},
        {"item": "mein iarainn", "weight": 10, "min": 1, "max": 3}, {"item": "luibheanna", "weight": 10, "min": 1, "max": 3}]},
    {"name": "creach mheanach", "rolls": 1, "nothing": 60, "entries": [
        {"item": "claiomh iarainn", "weight": 5}, {"item": "tua iarainn", "weight": 5}, {"item": "claiomh mor", "weight": 3}, {"item": "lorg dharach", "weight": 3},
        {"item": "slat iubhair", "weight": 2}, {"item": "miodog", "weight": 3}, {"item": "cota fionnaidh", "weight": 3}, {"item": "luireach", "weight": 3},
        {"item": "torc oir", "weight": 2}, {"item": "barra iarainn", "weight": 10, "min": 1, "max": 3}, {"item": "deoch leighis mhor", "weight": 6}, {"item": "bonn oir", "weight": 4}]},
    {"name": "creach ard", "rolls": 1, "nothing": 50, "entries": [
        {"item": "claiomh cruach", "weight": 5}, {"item": "claiomh lochlannach", "weight": 2}, {"item": "tua lochlannach", "weight": 2}, {"item": "claiomh mor na laoch", "weight": 2},
        {"item": "lorg cuilinn", "weight": 2}, {"item": "crosbhogha trom", "weight": 3}, {"item": "luireach throill", "weight": 2}, {"item": "cloch nathrach", "weight": 2},
        {"item": "briocht fiaigh", "weight": 2}, {"item": "deoch chosanta", "weight": 4}, {"item": "deoch laoich", "weight": 4}, {"item": "bonn oir", "weight": 8, "min": 1, "max": 3}]},
    {"name": "creach eipiciuil", "rolls": 2, "nothing": 20, "entries": [
        {"item": "luireach lann", "weight": 3}, {"item": "fiacail fhomhorach", "weight": 3}, {"item": "lunula oir", "weight": 2}, {"item": "crosbhogha an fhiaga", "weight": 1},
        {"item": "slat na si", "weight": 1}, {"item": "lorg na ndrui", "weight": 1}, {"item": "tua mhor huscarla", "weight": 1}, {"item": "luireach huscarla", "weight": 1},
        {"item": "capall cogaidh", "weight": 1}, {"item": "bonn oir", "weight": 6, "min": 2, "max": 5}]},
]
LOOT_OF = {
    "mac tire": "creach iseal", "dibheirgeach": "creach iseal", "meirleach": "creach iseal", "torc": "creach iseal", "beacanan": "creach iseal",
    "solas si": "creach iseal", "ceithearnach": "creach iseal", "puca": "creach mheanach", "bear": "creach iseal",
    "fomhorach": "creach mheanach", "troll cnoic": "creach mheanach", "creachadoir": "creach mheanach", "damhan alla mor": "creach mheanach",
    "troll sneachta": "creach ard", "bean si": "creach ard", "gruagach": "creach ard", "dubhlachan": "creach ard", "ri na mbeacan": "creach ard",
    "curadh fomhorach": "creach eipiciuil", "peist locha": "creach eipiciuil", "taibhse uaimhe": "creach mheanach", "huscarl": "creach ard",
}
TROPHIES = {
    "crom dubh": [{"item": "croi chrom dubh", "chance_pct": 100.0, "min": 1, "max": 1}],
    "ri na ndibheirgeach": [{"item": "coroin an ri", "chance_pct": 100.0, "min": 1, "max": 1}],
    "an morrigan": [{"item": "cleite na morrigan", "chance_pct": 100.0, "min": 1, "max": 1}, {"item": "cleite fiaigh", "chance_pct": 100.0, "min": 2, "max": 4}],
    "gofraidh": [{"item": "fainne an iarla", "chance_pct": 100.0, "min": 1, "max": 1}],
    "peist locha": [{"item": "fiacail peiste", "chance_pct": 35.0, "min": 1, "max": 1}, {"item": "lann peiste", "chance_pct": 90.0, "min": 1, "max": 3}],
    "huscarl": [{"item": "barra iarainn", "chance_pct": 60.0, "min": 1, "max": 2}],
    "troll cnoic": [{"item": "craiceann trolla", "chance_pct": 45.0, "min": 1, "max": 1}],
    "troll sneachta": [{"item": "craiceann trolla", "chance_pct": 60.0, "min": 1, "max": 2}],
    "fomhorach": [{"item": "cnamh fomhorach", "chance_pct": 20.0, "min": 1, "max": 1}],
    "fia": [{"item": "feoil", "chance_pct": 60.0, "min": 1, "max": 2}],
    "damh": [{"item": "feoil", "chance_pct": 80.0, "min": 1, "max": 3}],
    "cearc": [{"item": "ubh", "chance_pct": 50.0, "min": 1, "max": 2}],
    "caora": [{"item": "olann", "chance_pct": 70.0, "min": 1, "max": 2}],
    "loscann": [{"item": "luibheanna", "chance_pct": 25.0, "min": 1, "max": 1}],
    "beacanan": [{"item": "luibheanna", "chance_pct": 60.0, "min": 1, "max": 2}],
    "taibhse uaimhe": [{"item": "cleite fiaigh", "chance_pct": 30.0, "min": 1, "max": 2}],
    "iascaire": [{"item": "bradan", "chance_pct": 40.0, "min": 1, "max": 1}],
}

# ------------------------------------------------------------------------------------------------ quests


def q(name, title, giver, text, done, steps, xp, reward=(), **kw):
    d = {"name": name, "title": title, "text": text, "done_text": done, "giver": giver, "steps": steps, "xp": xp}
    if reward:
        d["reward"] = [{"item": i, "count": c} for i, c in reward]
    d.update(kw)
    return d


def kill(bot, n):
    return {"kind": "kill", "archetype": bot, "count": n}


def get(item, n):
    return {"kind": "collect", "item": item, "count": n}


def visit(place):
    return {"kind": "visit", "place": place}


def talk(bot):
    return {"kind": "talk", "archetype": bot}


P = "pingin airgid"
QUESTS = [
    # The smith's chain: from ore to steel.
    q("ore for the forge", "Ore for the Forge", "gabha", "The forge eats iron faster than the carters bring it. The kern up in the hills carry ore they've robbed off the miners. Bring me six lumps, from them or anywhere.", "Good ore. You'll make a smith yet.", [get("mein iarainn", 6)], 120, [(P, 6), ("tua bheag", 1)]),
    q("the smiths apprentice", "The Smith's Apprentice", "gabha", "Smelt it yourself this time: two ore and a lump of charcoal make a bar. Charcoal is two of timber. Bring me three bars and I'll show you a sword.", "Clean bars. Here, make yourself a blade.", [get("barra iarainn", 3)], 200, [("claiomh iarainn", 1)], after="ore for the forge", level=3),
    q("hides for a coat", "Hides for a Coat", "gabha", "A troll's hide turns a blade like nothing else. The hill trolls on Sliabh Speirín wear the best of it. Bring me three.", "Thick as a door. I'll have a coat out of this.", [get("craiceann trolla", 3)], 600, [("luireach throill", 1)], after="the smiths apprentice", level=9),
    q("scales of the wyrm", "Scales of the Wyrm", "gabha", "Now, there's a thing I've only heard of: a hauberk of wyrm scale. The péist in the great lochs sheds them. Four, and I'll make it.", "Green as the sea and hard as stone. Wear it well.", [kill("peist locha", 1), get("lann peiste", 4)], 1400, [("luireach lann", 1)], after="hides for a coat", level=13),
    q("the answerer", "The Answerer", "gabha", "There was a sword once, Fragarach, that no armour could stop. Manannán's own. Bring me the heart of Crom Dubh and the tooth of a great wyrm, and I'll try to make it again.", "It answers. God help whoever asks it a question.", [get("croi chrom dubh", 1), get("fiacail peiste", 1)], 4000, [("fragarach", 1)], after="scales of the wyrm", level=19),
    # The druid's chain.
    q("holly for a staff", "Holly for a Staff", "drui", "Holly keeps its leaves when the oak gives up. Cut me four branches: the shroomlings in Coill na Mac Tíre guard the best of it.", "Holly. Good. The staff will remember who cut it.", [kill("beacanan", 8), get("cuileann", 4)], 300, [("lorg dharach", 1)], level=5),
    q("the crows of the morrigan", "The Crows of the Morrígan", "drui", "The Morrígan's crows are thick over Cruachan. Bring me six raven feathers, and I'll read what they say.", "They say she is waking. That is not good news.", [get("cleite fiaigh", 6)], 700, [("briocht fiaigh", 1)], after="holly for a staff", level=10),
    q("the stone that roars", "The Stone That Roars", "drui", "The Lia Fáil is silent because the old powers are loose. Crom Dubh in his mound, the Morrígan in her cave. Bring me something of each, and I'll wake the stone.", "It roars again. Ireland has a king, whether it knows it or not.", [get("croi chrom dubh", 1), get("cleite na morrigan", 1)], 5000, [("slis lia fail", 1)], after="the crows of the morrigan", level=18),
    q("the sidhe at the door", "The Sídhe at the Door", "drui", "The bean sí keen at the old forts at night. One sings for every death. Silence three of them, before they sing for you.", "Quiet. For now.", [kill("bean si", 3)], 1600, [("slat na si", 1)], after="the crows of the morrigan", level=12),
    # The storyteller's tales of the Fianna.
    q("the salmon of knowledge", "The Salmon of Knowledge", "seanchai", "Fionn burnt his thumb on the Salmon of Knowledge and knew everything after. I'm not asking for that one. Any salmon will do: bring me two, and I'll tell you the rest.", "...and that's why he sucked his thumb before every battle. Thank you, child.", [get("bradan", 2)], 150, [(P, 5)]),
    q("the hill of allen", "The Hill of Allen", "seanchai", "The Fianna gathered at Almhain. Walk to Teamhair, then to Dún Ailinne, and you'll have walked where they walked.", "You've the feet of a fénnid, I'll say that.", [visit("tara"), visit("dun ailinne")], 300, [(P, 10)], level=3),
    q("oisin in the west", "Oisín in the West", "seanchai", "Oisín went west to Tír na nÓg, they say, from the cliffs past Gaillimh. Go and look out to sea from there, and tell me if you see it.", "You saw nothing? Then you looked right. It isn't for seeing.", [visit("galway"), visit("westport")], 350, [(P, 12)], after="the hill of allen", level=4),
    q("the giant under the mountain", "The Giant Under the Mountain", "seanchai", "Fionn fought a gruagach under Earagail, a great hairy brute who stole cattle. There are still some of them up there. Bring me proof one fell.", "A gruagach! My own grandfather never saw one. You're in my next story.", [kill("gruagach", 1)], 1500, [("torc oir", 1)], after="oisin in the west", level=12),
    q("the headless rider", "The Headless Rider", "seanchai", "Nobody likes this one. A rider with his head under his arm, who says your name and you die. They ride the black passes. If you'll stop one, I'll stop telling it.", "Then I'll tell a new one, with you in it.", [kill("dubhlachan", 1)], 2000, [("lunula oir", 1)], after="the giant under the mountain", level=14),
    # The herb-wife.
    q("herbs for the sick", "Herbs for the Sick", "bean feasa", "Half the village is coughing. Bring me eight bundles of herbs: the frogs in the lochs and the shroomlings both carry them.", "That'll do them. God bless you.", [get("luibheanna", 8)], 140, [("deoch leighis", 3)]),
    q("honey for the cure", "Honey for the Cure", "bean feasa", "The greater draught wants honey. The bakers keep it. Two jars, and I'll show you the brew.", "Sweet as anything. Here, carry a few.", [get("mil", 2)], 220, [("deoch leighis mhor", 2)], after="herbs for the sick", level=4),
    q("the wisps of the bog", "The Wisps of the Bog", "bean feasa", "Lights over the loch at night, leading folk into the water. Put out six of them.", "No more drownings, please God.", [kill("solas si", 6)], 400, [(P, 10), ("deoch leighis mhor", 1)], after="herbs for the sick", level=4),
    q("silk for the stitching", "Silk for the Stitching", "bean feasa", "Spider silk closes a wound better than gut. The great spiders in the caves spin it. Four threads.", "Strong and fine. You've saved a life with that, somewhere.", [kill("damhan alla mor", 4), get("sioda damhain", 4)], 700, [("deoch chosanta", 2)], after="the wisps of the bog", level=7),
    # The hunter.
    q("hare today", "Hare Today", "sealgaire", "There's a fox has been taking my snares. Take six hares before it does, and bring me the meat.", "Good shooting. We'll eat tonight.", [kill("giorria", 6), get("feoil", 4)], 100, [("feoil rosta", 3)]),
    q("the badger sett", "The Badger Sett", "sealgaire", "Badgers under the hedges, and the hedges falling down. Bring me four pelts and I'll make you a jerkin.", "Fine pelts. Here's your jerkin.", [get("craiceann broic", 4)], 160, [("seaicead leathair", 1)], after="hare today"),
    q("the bear of the wood", "The Bear of the Wood", "sealgaire", "A bear has come down out of the hills and killed a cow. Bring it down. Two of its pelts, if there's more than one.", "Two pelts. There'll be a fur coat out of these.", [kill("bear", 2), get("craiceann beir", 2)], 450, [("cota fionnaidh", 1)], after="the badger sett", level=5),
    q("the bull in the low field", "The Bull in the Low Field", "sealgaire", "The wild bulls are worse than any wolf. Bring three of them down and bring me their horns.", "Horns like that, a smith will pay well. Keep one.", [kill("tarbh fiain", 3), get("adharc tairbh", 3)], 500, [("miodog", 1)], after="the badger sett", level=5),
    q("the huntsmans bow", "The Huntsman's Bow", "sealgaire", "There's a crossbow in the old stories that never missed. I can make one, with a wyrm's tooth for the nut and spider silk for the string.", "There. It won't miss, if you don't.", [get("fiacail peiste", 1), get("sioda damhain", 3)], 2500, [("crosbhogha an fhiaga", 1)], after="the bear of the wood", level=15),
    # The baker and the miller.
    q("flour for the oven", "Flour for the Oven", "baicear", "The miller's late again. Bring me six sheaves of wheat and I'll bake for the whole street.", "Smell that! Take a loaf or three.", [get("cruithneacht", 6)], 100, [("aran", 4), ("cais", 1)]),
    q("eggs and honey", "Eggs and Honey", "baicear", "For the honey cakes: three eggs from the farms, and a jar of honey.", "There's your cake. Don't eat it all at once.", [get("ubh", 3), get("mil", 1)], 140, [("caca meala", 3)], after="flour for the oven"),
    q("the millers wheel", "The Miller's Wheel", "muilleoir", "Something stops my wheel every night: a little red man, laughing. The far darrig. Catch three and they'll leave me be.", "Turning all night, and not a laugh. Thank you.", [kill("fear dearg", 3)], 380, [(P, 12), ("cno", 5)], level=4),
    q("grist for the mill", "Grist for the Mill", "muilleoir", "Bring me oats, ten sheaves, and I'll grind them for the baker. She pays me in cake.", "She'll pay you in cake too, if you ask.", [get("coirce", 10)], 120, [(P, 6), ("cais", 2)]),
    # The weaver and the tinker.
    q("wool from the hills", "Wool from the Hills", "figheadoir", "Five fleeces from the farms. The sheep won't mind, much.", "Lovely and soft. Here, a tunic for your trouble.", [get("olann", 5)], 90, [("ionar olla", 1)]),
    q("the royal purple", "The Royal Purple", "figheadoir", "Purple is for kings. I can make it, with spider silk and a gold piece. Bring me both.", "There. Fit for a king. Or you.", [get("sioda damhain", 2), get("bonn oir", 1)], 400, [("corcra", 1)], after="wool from the hills", level=7),
    q("a pot to mend", "A Pot to Mend", "tinceir", "I need a little iron for the mending. Two bars, and I'll give you a brooch I found on the road.", "Found on the road, I said. Don't ask where.", [get("barra iarainn", 2)], 150, [("dealg chre-umha", 1), (P, 4)]),
    q("the leprechauns gold", "The Leprechaun's Gold", "tinceir", "Little green fellows with pots of gold, in the woods by the lochs. Catch me two and I'll split the gold with you.", "Not a pot. A few coins. Still! Here's your half.", [kill("leipreachan", 2), get("bonn oir", 2)], 450, [("bonn oir", 2), ("torc cre-umha", 1)], level=4),
    q("the long road", "The Long Road", "tinceir", "I'll tell you where I've been: Doire, Béal Feirste, Corcaigh, Trá Lí. You go and see if I'm lying.", "Told you. Have this, it's a fine pony.", [visit("derry"), visit("belfast"), visit("cork"), visit("tralee")], 900, [("capaillin", 1)], level=5),
    # The priest.
    q("candles for the dead", "Candles for the Dead", "sagart", "The dead are restless at Brú na Bóinne. Lay them to rest: ten of the sluagh.", "May they sleep now.", [kill("sluagh", 10)], 700, [("deoch leighis mhor", 2), (P, 10)], level=6),
    q("the black passes", "The Black Passes", "sagart", "Pilgrims won't take the road through Bealach na gCreach since the kern came. Clear eight of them out.", "The pilgrims will pray for you.", [kill("ceithearnach", 8)], 650, [(P, 15), ("deoch laoich", 1)], level=6),
    # Monster hunts by region, from the watch and the arms trader.
    q("the puca", "The Púca", "watchman", "A black horse with yellow eyes, carrying folk off at night. They call it the púca. Bring three down.", "Three fewer to fear. You're a brave one.", [kill("puca", 3)], 700, [(P, 15), ("deoch laoich", 1)], level=7),
    q("cattle raiders", "Cattle Raiders", "laoch", "Raiders down out of the mountains, running off whole herds. Ten of them, and the herds come home.", "The farmers will drink to you tonight.", [kill("creachadoir", 10)], 1100, [("crosbhogha trom", 1)], level=9),
    q("the fomorian champion", "The Fomorian Champion", "laoch", "The Fomorians have a champion in the north, twice the size of the rest. Bring him down, and bring back a bone of him.", "Like a tree trunk! Ireland's a little safer.", [kill("curadh fomhorach", 1), get("cnamh fomhorach", 1)], 2600, [("claiomh mor na laoch", 1)], level=15),
    q("north wind", "North Wind", "laoch", "Snow trolls coming down further every winter. Twelve of them, before they reach the farms.", "Twelve! The north will be quieter.", [kill("troll sneachta", 12)], 1500, [("cloch nathrach", 1)], level=11),
    q("the wolves of winter", "The Wolves of Winter", "watchman", "White wolves out of the north, bolder every week. Twenty of them.", "That'll keep them back a season.", [kill("mac tire ban", 20)], 900, [("cota fionnaidh", 1)], level=4, daily=True),
    q("trollhunter", "Trollhunter", "gabha", "I pay by the hide. Any troll. Bring me what you can.", "Good hides. Here's your pay.", [get("craiceann trolla", 2)], 300, [(P, 20)], level=8, repeatable=True),
]


def town_errands(towns, places, g):
    """An errand for every big town: word carried to a neighbour, a threat near it, supplies for it."""
    pos = {p["name"]: (p["x"], p["y"]) for p in g["places"]}
    out = []
    kinds = [
        ("word", "watchman"), ("supplies", "ceannai"), ("hunt", "watchman"), ("word", "tuathanach"),
    ]
    threats = ["mac tire", "torc", "dibheirgeach", "ceithearnach", "bear", "puca", "solas si", "creachadoir"]
    for i, t in enumerate(towns[1:], 1):
        if t not in pos:
            continue
        x, y = pos[t]
        near = sorted((n for n in towns if n != t and n in pos), key=lambda n: (pos[n][0] - x) ** 2 + (pos[n][1] - y) ** 2)
        nb = near[0] if near else "dublin"
        tt, nt = TOWN_TITLES.get(t, t.title()), TOWN_TITLES.get(nb, nb.title())
        kind, giver = kinds[i % len(kinds)]
        lvl = 1 + i % 12
        if kind == "word":
            out.append(q(f"word to {nb} from {t}", f"Word to {nt}", giver,
                         f"There's news for {nt} that won't keep. Walk to {tt}, then on to {nt}, and tell them what you've seen on the road.",
                         "That's the word carried. They'll be glad of it.", [visit(t), visit(nb)], 150 + 30 * lvl, [(P, 4 + lvl)], level=lvl))
        elif kind == "supplies":
            food_ = ["aran", "cais", "feoil rosta", "bradan", "meacan", "cabaiste"][i % 6]
            out.append(q(f"supplies for {t}", f"Supplies for {tt}", giver,
                         f"{tt} is short this season. Carry five of {food_} there and see the market stocked.",
                         "Stocked, and the price come down. Well done.", [get(food_, 5), visit(t)], 120 + 25 * lvl, [(P, 6 + lvl)], level=lvl))
        else:
            th = threats[i % len(threats)]
            out.append(q(f"the road to {t}", f"The Road to {tt}", giver,
                         f"Travellers to {tt} keep coming back hurt, or not at all. Clear the road: eight of what's out there, and go and see the town safe.",
                         "The road's open. The carters will thank you.", [kill(th, 8), visit(t)], 200 + 40 * lvl, [(P, 8 + lvl), ("deoch leighis", 2)], level=lvl))
    return out


ACHIEVEMENTS = [
    {"name": "bearbane", "title": "Bearbane", "text": "Ten bears.", "deed": "kill:bear", "count": 10, "grants": "Bearbane", "xp": 150},
    {"name": "headsman", "title": "Headsman", "text": "A dullahan, brought down.", "deed": "kill:dubhlachan", "count": 1, "grants": "the Headsman", "xp": 300},
    {"name": "giantkiller", "title": "Giant-killer", "text": "Five gruagach.", "deed": "kill:gruagach", "count": 5, "grants": "Giant-killer", "xp": 300},
    {"name": "lucky", "title": "Lucky", "text": "A leprechaun caught.", "deed": "kill:leipreachan", "count": 1, "grants": "the Lucky", "xp": 80},
    {"name": "wisp-quencher", "title": "Wisp-quencher", "text": "Twenty-five wisps put out.", "deed": "kill:solas si", "count": 25, "grants": "the Lantern", "xp": 150},
    {"name": "champion", "title": "Champion of Ireland", "text": "The Fomorian Champion, beaten.", "deed": "kill:curadh fomhorach", "count": 1, "grants": "Champion", "xp": 400},
]

if __name__ == "__main__":
    main()

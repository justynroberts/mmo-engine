#!/usr/bin/env python3
"""Drive the built UI in a real browser against a real server and check it works.

Usage: scripts/ui_e2e.py [--shots DIR]
Needs: cargo build --release, (cd ui && npm run build), python3 playwright + chromium.
Starts its own server on free ports with a throwaway game.json and key.
"""
import json, os, re, shutil, signal, socket, subprocess, sys, tempfile, time, urllib.request
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "target/release/server"
BOTS = ROOT / "target/release/bots"
AXE = ROOT / "ui/node_modules/axe-core/axe.min.js"
SHOTS = None
if "--shots" in sys.argv:
    SHOTS = Path(sys.argv[sys.argv.index("--shots") + 1]); SHOTS.mkdir(parents=True, exist_ok=True)

results = []
def check(name, ok, detail=""):
    results.append((name, bool(ok), detail))
    print(("PASS " if ok else "FAIL ") + name + (f"  -- {detail}" if detail and not ok else ""), flush=True)

def free_port(udp=False):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM if udp else socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p

def api(base, path, method="GET", body=None):
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"X-Admin": "1", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r: return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"{}")

work = Path(tempfile.mkdtemp(prefix="ui-e2e-"))
udp, http, webp = free_port(True), free_port(), free_port()
web = f"http://127.0.0.1:{webp}"
base = f"http://127.0.0.1:{http}"
srv = subprocess.Popen([str(SERVER), "--port", str(udp), "--admin", f"127.0.0.1:{http}", "--ui-dir", str(ROOT / "ui/dist"),
                        "--world-size", "1024", "--quiet", "--timeout", "30", "--web", f"127.0.0.1:{webp}"], cwd=work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
bots = None
try:
    for _ in range(100):
        try: api(base, "/api/info"); break
        except Exception: time.sleep(0.1)
    key = api(base, "/api/info")[1]["server_key"]
    bots = subprocess.Popen([str(BOTS), "--server", f"127.0.0.1:{udp}", "--server-key", key, "--bots", "60", "--duration", "300",
                             "--quiet", "--game", str(work / "game.json")], cwd=work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)
    st = api(base, "/api/state")[1]
    check("server has players from encrypted bots", st["clients"] >= 50, f"clients={st['clients']}")

    # The console's sections and the pages inside them, by the names the old flat tabs had.
    PAGES = {"Bots": ("Bots", "Bot types"), "Spawn": ("Bots", "Spawn and scatter"), "Builder": ("World", "Places, routes and walls"),
             "Terrain": ("World", "Terrain"), "Attributes": ("Rules", "Attributes"), "Actions": ("Rules", "Actions"), "Items": ("Rules", "Items"),
             "Entity": ("Live", "Inspect"), "Chat": ("Live", "Chat"), "Connect": ("Server", "Connect"), "Network": ("Server", "Network"),
             "JSON": ("Server", "Game file (JSON)"), "Sky": ("Sky", None), "Events": ("Events", None)}

    def open_page(pg, name):
        section, sub = PAGES[name]
        pg.get_by_role("tab", name=section, exact=True).click()
        if sub:
            pg.get_by_role("tab", name=sub, exact=True).click()
        time.sleep(0.2)

    with sync_playwright() as pw:
        br = pw.chromium.launch(args=["--no-sandbox", "--use-gl=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"])
        TABS = ["Bots", "Spawn", "Terrain", "Builder", "Sky", "Events", "Attributes", "Actions", "Items", "Entity", "Chat", "Connect", "Network", "JSON"]
        for theme in ("dark",):  # the console is near-black only
            ctx = br.new_context(viewport={"width": 1440, "height": 900}, color_scheme=theme)
            pg = ctx.new_page()
            errors = []
            pg.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            pg.on("pageerror", lambda e: errors.append(str(e)))
            pg.goto(base + "/")
            pg.wait_for_selector("text=Server running", timeout=8000)
            check(f"[{theme}] page loads live with no console errors", not errors, str(errors))
            check(f"[{theme}] the console is dark", "dark" in pg.evaluate("document.documentElement.className"))
            for t in TABS:
                open_page(pg, t)
                time.sleep(0.1)
                pg.add_script_tag(path=str(AXE)) if not pg.evaluate("!!window.axe") else None
                res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,impact:v.impact,n:v.nodes.length,t:v.nodes.slice(0,3).map(n=>n.target.join(' ')+' :: '+(n.failureSummary||'').split('\\n').slice(0,3).join(' | '))})))")
                check(f"[{theme}] accessibility, {t} tab: no axe violations", not res, json.dumps(res)[:900])
                if SHOTS: pg.screenshot(path=str(SHOTS / f"{theme}-{t.lower()}.png"))
            ctx.close()

        # --- behaviour tests (dark theme, desktop)
        ctx = br.new_context(viewport={"width": 1440, "height": 900}, color_scheme="dark")
        pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=Server running")
        time.sleep(1)

        # Canvas click selects an entity (initial view is fitted, so screen position is computable).
        box = pg.locator("canvas").bounding_box()
        scale = min(box["width"], box["height"]) / 1024.0
        pg.wait_for_timeout(300)
        st = api(base, "/api/state")[1]   # fetch right before clicking: entities move
        picked = None
        for row in st["e"]:
            x = box["x"] + (row[1] - 512) * scale + box["width"] / 2
            y = box["y"] + (row[2] - 512) * scale + box["height"] / 2
            others = [r for r in st["e"] if r is not row and abs(r[1]-row[1]) < 25 and abs(r[2]-row[2]) < 25]
            if not others and box["x"] < x < box["x"] + box["width"] and box["y"] < y < box["y"] + box["height"]:
                picked = (row, x, y); break
        if picked:
            pg.mouse.click(picked[1], picked[2]); pg.wait_for_timeout(500)
            sid = picked[0][0] & 0xFFFFFF
            check("clicking an entity on the map selects it and opens the Entity tab", pg.locator(f"h2:has-text('#{sid}')").count() == 1)
        else:
            check("clicking an entity on the map selects it", False, "no isolated entity found")

        # Bot-only edit applies live and changes the population.
        open_page(pg, "Bots")
        before = api(base, "/api/state")[1]
        cnt = pg.get_by_label("Spawn count").first
        cnt.fill("20")
        check("Apply button enables when the draft differs", pg.get_by_role("button", name="Apply", exact=True).is_enabled())
        check("bot-only change says it applies live", pg.locator("text=they apply live").count() == 1)
        pg.get_by_role("button", name="Apply", exact=True).click()
        pg.wait_for_selector("text=Applied live", timeout=5000)
        time.sleep(0.6)
        after = api(base, "/api/state")[1]
        drifters = sum(1 for r in after["e"] if r[3] == 0)
        check("applying a bot change spawned the new population", drifters == 20, f"drifters={drifters}")
        check("a live bot change does not drop players", after["clients"] >= before["clients"] - 2, f"{before['clients']} -> {after['clients']}")
        check("alive counter in the UI updates", pg.locator("text=20 alive").first.is_visible())

        # Spawn button.
        pg.get_by_role("button", name="Spawn 10").first.click(); time.sleep(0.6)
        n2 = sum(1 for r in api(base, "/api/state")[1]["e"] if r[3] == 0)
        check("Spawn 10 adds ten bots", n2 == 30, f"{n2}")

        # ---- Visual builder: places, routes, rules, test bot, all through the real UI.
        open_page(pg, "Builder"); time.sleep(0.4)
        cv = pg.locator("canvas[role=img]")
        def cpos(wx, wy):
            bb = cv.bounding_box(); sc = min(bb["width"], bb["height"]) / 1024
            return bb["x"] + bb["width"] / 2 + (wx - 512) * sc, bb["y"] + bb["height"] / 2 + (wy - 512) * sc
        def field(name, n=0): return pg.get_by_role("textbox", name=name, exact=True).nth(n)
        n_places = pg.get_by_role("textbox", name="Place", exact=True).count()
        pg.get_by_role("button", name="Add place").click()
        pg.mouse.click(*cpos(600, 600)); time.sleep(0.3)
        check("Add place tool: clicking the map adds a place", pg.get_by_role("textbox", name="Place", exact=True).count() == n_places + 1)
        px = float(field("X", n_places).input_value())
        check("the new place sits where the map was clicked", abs(px - 600) <= 4, f"x={px}")
        a = cpos(600, 600); b = cpos(700, 650)
        pg.mouse.move(*a); pg.mouse.down(); pg.mouse.move((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, steps=4); pg.mouse.move(*b, steps=4); pg.mouse.up(); time.sleep(0.3)
        px, py = float(field("X", n_places).input_value()), float(field("Y", n_places).input_value())
        check("dragging a place on the map moves it", abs(px - 700) <= 5 and abs(py - 650) <= 5, f"({px},{py})")
        n_routes = pg.get_by_role("textbox", name="Route", exact=True).count()
        pg.get_by_role("button", name="Draw route").click()
        for pt in [(300, 800), (500, 850), (650, 800)]:
            pg.mouse.click(*cpos(*pt)); time.sleep(0.1)
        pg.get_by_role("button", name="Finish route").click(); time.sleep(0.3)
        check("Draw route tool: finishing adds a route", pg.get_by_role("textbox", name="Route", exact=True).count() == n_routes + 1)
        check("route has the clicked points", pg.locator("text=3 points").count() >= 1)

        pg.get_by_label("Editing bot type").select_option(label="guard")
        pg.get_by_label("Home").select_option("place")
        pg.get_by_label("Spawn count").fill("2")
        pg.get_by_role("button", name="Add rule").click(); time.sleep(0.2)
        pg.get_by_role("button", name="Move rule 3 up").click(); pg.get_by_role("button", name="Move rule 2 up").click(); time.sleep(0.3)
        r1 = pg.get_by_role("group", name=re.compile("^Rule 1:"))
        r1.get_by_label("Condition").select_option("chance")
        pg.get_by_role("group", name=re.compile("^Rule 1:")).get_by_label("Percent per decision").fill("100")
        pg.get_by_role("group", name=re.compile("^Rule 1:")).get_by_label("Action").select_option("say")
        pg.get_by_role("group", name=re.compile("^Rule 1:")).get_by_label("Text").fill("oyez")
        check("rules read back as sentences", pg.get_by_role("group", name=re.compile("^Rule 1: 100% of the time: say \"oyez\"")).count() == 1)
        check("rule reorder buttons act on the list", pg.get_by_role("group", name=re.compile("^Rule 3: Always: patrol")).count() == 1)
        pg.get_by_role("button", name="Apply", exact=True).click()
        pg.wait_for_selector("text=Applied live", timeout=5000)
        time.sleep(1.5)
        g = api(base, "/api/game")[1]
        guard = next(a for a in g["archetypes"] if a["name"] == "guard")
        check("applied game has the rule, home and place", guard["rules"][0]["then"]["kind"] == "say" and guard["home"] == "place" and any(p["name"] == "place" and abs(p["x"] - 700) <= 5 for p in g["places"]), json.dumps(guard)[:300])
        gi = [a["name"] for a in g["archetypes"]].index("guard")
        guards = [r for r in api(base, "/api/state")[1]["e"] if r[3] == gi]
        near = [r for r in guards if ((r[1] - 700) ** 2 + (r[2] - 650) ** 2) ** 0.5 < 80]
        check("two guards spawned in the new place and are patrolling or standing near it", len(guards) == 2 and len(near) >= 1, f"{len(guards)} guards, {[(round(r[1]), round(r[2])) for r in guards]}")
        lines = []
        for _ in range(20):
            lines = api(base, "/api/chat")[1]["lines"]
            if any(l[3] == "oyez" for l in lines): break
            time.sleep(0.3)
        check("a rule made bots speak through the chat channel", any(l[3] == "oyez" for l in lines), str(lines[-3:]))
        open_page(pg, "Chat"); time.sleep(1.4)
        check("the Chat tab shows what bots said", pg.locator("li:has-text('oyez')").count() >= 1)

        # ---- Map editor: draw a wall on the map, apply it live, hand the walls back to the generator.
        open_page(pg, "Builder"); time.sleep(0.4)
        n_generated = len(api(base, "/api/world")[1]["boxes"])
        pg.get_by_role("button", name="Draw wall").click()
        a = cpos(40, 960); b2 = cpos(140, 1000)
        pg.mouse.move(*a); pg.mouse.down(); pg.mouse.move(*b2, steps=6); pg.mouse.up(); time.sleep(0.3)
        check("Draw wall: dragging on the map adds a wall, taking over the generated ones", pg.get_by_role("textbox", name="Wall 1 X0", exact=True).count() == 1 and pg.locator(f"text={n_generated + 1} wall(s)").count() == 1, f"generated {n_generated}")
        pg.get_by_role("button", name="Apply", exact=True).click(); pg.wait_for_selector("text=Applied live", timeout=5000); time.sleep(0.8)
        boxes = api(base, "/api/world")[1]["boxes"]
        check("the applied wall is in the world where it was drawn", len(boxes) == n_generated + 1 and any(abs(w[0] - 40) <= 6 and abs(w[3] - 1000) <= 6 for w in boxes), str(boxes[-1:]))
        pg.get_by_role("button", name="Delete wall %d" % (n_generated + 1)).click(); time.sleep(0.3)
        check("deleting a wall removes it from the list", pg.locator(f"text={n_generated} wall(s)").count() == 1)
        pg.get_by_role("button", name="Back to generated").click(); time.sleep(0.3)
        pg.get_by_role("button", name="Apply", exact=True).click(); pg.wait_for_selector("text=Applied live", timeout=5000); time.sleep(0.8)
        check("back to generated walls restores the seed's world", len(api(base, "/api/world")[1]["boxes"]) == n_generated)
        open_page(pg, "Builder"); time.sleep(0.3)
        pg.get_by_role("button", name="Drop test bot").click()
        pg.mouse.click(*cpos(200, 200)); time.sleep(0.8)
        guards2 = [r for r in api(base, "/api/state")[1]["e"] if r[3] == gi]
        check("Drop test bot spawns the selected bot type at the click", len(guards2) == 3 and any(((r[1] - 200) ** 2 + (r[2] - 200) ** 2) ** 0.5 < 40 for r in guards2), f"{len(guards2)}")
        # Dragging a rule row by its handle reorders it.
        pg.get_by_role("button", name="Select and move").click()
        h1 = pg.get_by_role("group", name=re.compile("^Rule 1:")).bounding_box()
        h3 = pg.get_by_role("group", name=re.compile("^Rule 3:")).bounding_box()
        if h1 and h3:
            x0 = h1["x"] - 14; y0 = h1["y"] + 12
            pg.mouse.move(x0, y0); pg.mouse.down(); pg.mouse.move(x0, y0 + 20, steps=4); pg.mouse.move(x0, h3["y"] + h3["height"] + 6, steps=12); pg.mouse.up(); time.sleep(0.5)
            first = pg.get_by_role("group", name=re.compile("^Rule 1:")).get_attribute("aria-label")
            check("dragging a rule row reorders the stack", first is not None and "oyez" not in first, str(first))
        pg.get_by_role("button", name="Revert").click(); time.sleep(0.3)

        # Possession with the keyboard.
        open_page(pg, "Entity")
        bot = next(r for r in api(base, "/api/state")[1]["e"] if r[3] == 0)
        pg.get_by_role("button", name=f"#{bot[0] & 0xFFFFFF} drifter", exact=False).first.click()
        pg.get_by_role("button", name="Possess").click(); time.sleep(0.4)
        pg.keyboard.down("d"); time.sleep(1.2); pg.keyboard.up("d"); time.sleep(0.5)
        me = next(r for r in api(base, "/api/state")[1]["e"] if r[0] == bot[0])
        check("possessed entity is marked driven", me[4] == 2, str(me[:5]))
        check("WASD walks the possessed entity", abs(me[1] - bot[1]) > 3, f"{bot[1]} -> {me[1]}")
        pg.get_by_role("button", name="Release").click(); time.sleep(0.4)
        me = next(r for r in api(base, "/api/state")[1]["e"] if r[0] == bot[0])
        check("releasing hands the bot back to the AI", me[4] == 0, str(me[:5]))

        # Validation errors surface and nothing changes.
        open_page(pg, "Attributes")
        h0 = api(base, "/api/state")[1]["hash"]
        pg.get_by_label("Min", exact=True).first.fill("500")
        pg.get_by_role("button", name="Apply", exact=True).click()
        pg.wait_for_selector("[role=alert]", timeout=5000)
        check("invalid edit shows the server's error", "min must be below max" in pg.locator("[role=alert]").inner_text())
        check("invalid edit changed nothing on the server", api(base, "/api/state")[1]["hash"] == h0)
        pg.get_by_role("button", name="Revert").click()

        # Wire-format change warns, then restarts the world.
        pg.get_by_label("Max", exact=True).first.fill("250")
        check("wire-format change warns about the restart", pg.locator("text=restarts the world").count() == 1)
        pg.get_by_role("button", name="Apply", exact=True).click()
        pg.wait_for_selector("text=World restarted", timeout=5000)
        time.sleep(0.8)
        st2 = api(base, "/api/state")[1]
        check("content hash changed and players were dropped", st2["hash"] != h0 and st2["clients"] == 0, f"{st2['hash']} clients={st2['clients']}")
        check("edit was saved to game.json", '"max": 250' in (work / "game.json").read_text())

        # Raw JSON.
        open_page(pg, "JSON")
        ta = pg.locator("textarea")
        ta.fill("{ not json")
        check("invalid JSON shows an error", pg.locator("[role=alert]").count() >= 1)
        # Rename propagation through the form.
        open_page(pg, "Attributes")
        open_page(pg, "JSON")
        txt = ta.input_value()
        check("draft is restored from the last good state", '"attrs"' in txt)

        # The console is near-black only: no theme switch, still dark after a reload.
        pg.reload(); pg.wait_for_selector("text=Server running")
        check("the console has no theme switch and stays dark", pg.get_by_role("button", name="Switch to light theme").count() == 0
              and "dark" in pg.evaluate("document.documentElement.className"))
        ctx.close()

        # Keyboard-only: arrow keys move between tabs (Radix roving focus).
        ctx = br.new_context(viewport={"width": 1440, "height": 900}); pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=Server running")
        pg.get_by_role("tab", name="Bots", exact=True).focus(); pg.keyboard.press("ArrowRight"); pg.wait_for_timeout(300)
        check("arrow keys switch tabs", pg.get_by_role("tab", name="Sky", exact=True).get_attribute("data-state") == "active")
        ctx.close()

        # 3D view: renders, lists entities, selects on click, switches back.
        ctx = br.new_context(viewport={"width": 1440, "height": 900}); pg = ctx.new_page()
        errs3 = []; pg.on("pageerror", lambda e: errs3.append(str(e)))
        pg.goto(base + "/"); pg.wait_for_selector("text=Server running")
        pg.get_by_role("button", name="3D", exact=True).click(); pg.wait_for_timeout(2500)
        legend = pg.get_by_role("list", name="Legend").inner_text()
        check("3D view shows a legend with live counts", "drifter" in legend and "players" in legend, legend.replace("\n", " "))
        check("3D view has a labelled canvas", pg.get_by_role("img", name="3D view of the world").count() == 1)
        lit = pg.evaluate("() => { const c = document.querySelector('canvas[aria-label^=\"3D\"]'); const t = document.createElement('canvas'); t.width = c.width; t.height = c.height; return c.width > 0 }")
        check("3D canvas has a size", lit)
        check("3D view raised no page errors", not errs3, str(errs3))
        if SHOTS: pg.screenshot(path=str(SHOTS / "3d.png"))
        pg.get_by_role("button", name="2D", exact=True).click(); pg.wait_for_timeout(300)
        check("switching back to 2D restores the map", pg.get_by_role("img", name="Top-down map").count() == 1)
        ctx.close()

        # Weather (the Sky section), events and spawning (Bots), built and used from the console.
        ctx = br.new_context(viewport={"width": 1440, "height": 900}); pg = ctx.new_page()
        gerr = []; pg.on("pageerror", lambda e: gerr.append(str(e)))
        pg.goto(base + "/"); pg.wait_for_selector("text=Server running")
        open_page(pg, "Sky")
        pg.get_by_label("Add a weather from a preset").select_option("fog")
        check("a weather can be added from a preset", pg.get_by_label("Weather", exact=True).count() >= 1)
        open_page(pg, "Events")
        pg.get_by_label("Add an event from a preset").select_option("wave")
        check("an event can be added from a preset", pg.get_by_role("list", name="Steps of wave").count() == 1)
        check("a new event cannot be started before it is applied", pg.get_by_role("button", name="Start now").is_disabled())
        pg.get_by_label("Add an event from a preset").select_option("raid")
        check("a raid preset has a timeline and steps for failing", pg.get_by_role("list", name="Steps of raid").count() == 1 and pg.get_by_role("list", name="If raid fails").count() == 1)
        pg.add_script_tag(path=str(AXE))
        res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,n:v.nodes.length,t:v.nodes.slice(0,2).map(n=>n.target.join(' '))})))")
        check("Events page: no axe violations", not res, json.dumps(res)[:600])
        if SHOTS: pg.screenshot(path=str(SHOTS / "events.png"))
        c_before = api(base, "/api/state")[1]["clients"]
        pg.get_by_role("button", name="Apply", exact=True).click(); pg.wait_for_selector("text=Applied live", timeout=5000); time.sleep(0.8)
        check("weather and events apply live without dropping players", api(base, "/api/state")[1]["clients"] >= c_before - 2, f"{c_before} -> {api(base, '/api/state')[1]['clients']}")
        open_page(pg, "Sky")
        pg.get_by_role("button", name="fog", exact=True).click(); time.sleep(0.6)
        check("the fog button starts the weather", (api(base, "/api/state")[1].get("director", {}).get("weather") or {}).get("name") == "fog")
        pg.wait_for_selector("text=Now: fog", timeout=4000)
        pg.get_by_role("button", name="Clear sky").click(); time.sleep(0.6)
        check("Clear sky ends it", api(base, "/api/state")[1]["director"]["weather"] is None)
        n0 = api(base, "/api/state?limit=6000")[1]["live"]
        open_page(pg, "Events")
        pg.get_by_role("button", name="Start now").first.click(); time.sleep(1.0)
        st = api(base, "/api/state?limit=6000")[1]
        check("firing the wave event spawns bots", st["live"] >= n0 + 8, f"{n0} -> {st['live']}")
        check("the event's announcement is in the chat from the server", any(l[1] == 0 and "Intruders" in l[3] for l in api(base, "/api/chat")[1]["lines"]))
        check("the log lists what happened", "wave" in pg.get_by_role("region", name="What happened").inner_text())
        # The raid: it starts, the console shows it running (waiting before its first wave), and Stop ends it.
        pg.get_by_role("button", name="Start now").last.click()
        pg.wait_for_selector("text=Running: step", timeout=5000)
        runs = api(base, "/api/state")[1]["director"].get("runs", [])
        check("a started raid is running and waiting", any(r["name"] == "raid" and r["waiting"] for r in runs), json.dumps(runs))
        check("its warning went out as a banner", any(l[1] == 0 and l[2] == 3 and "Raiders" in l[3] for l in api(base, "/api/chat")[1]["lines"]))
        pg.get_by_role("button", name="Stop", exact=True).click(); time.sleep(1.2)
        check("Stop ends it", not api(base, "/api/state")[1]["director"].get("runs") and pg.get_by_text("last stopped").count() == 1)
        # Spawn by clicking the map.
        open_page(pg, "Spawn")
        pg.get_by_role("region", name="Spawn bots").get_by_label("Where").select_option("@map")
        pg.get_by_role("button", name="Arm the map").click()
        n1 = api(base, "/api/state?limit=6000")[1]["live"]
        m = pg.get_by_role("img", name="Top-down map"); box = m.bounding_box()
        m.click(position={"x": box["width"] * 0.5, "y": box["height"] * 0.5}); time.sleep(0.8)
        check("clicking the armed map drops bots there", api(base, "/api/state?limit=6000")[1]["live"] >= n1 + 5)
        check("the Sky, Events and Bots pages raised no page errors", not gerr, str(gerr))
        ctx.close()

        # The play page: sign in with a passphrase, walk, chat, and see the recovery code.
        ctx = br.new_context(viewport={"width": 1280, "height": 800}); pg = ctx.new_page()
        perr = []; pg.on("pageerror", lambda e: perr.append(str(e)))
        pg.on("console", lambda m: perr.append(m.text) if m.type == "error" else None)
        before = {e[0] for e in api(base, "/api/state?limit=6000")[1]["e"] if e[4] == 1}
        pg.goto(web + "/"); pg.get_by_role("button", name="Enter the world").wait_for(timeout=8000)
        pg.add_script_tag(path=str(AXE))
        res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,n:v.nodes.length,t:v.nodes.slice(0,2).map(n=>n.target.join(' '))})))")
        check("play page sign-in screen: no axe violations", not res, json.dumps(res)[:600])
        pg.get_by_label("Account name").fill("e2e-walker"); pg.get_by_label("Passphrase").fill("a very long passphrase for testing")
        pg.get_by_role("button", name="Enter the world").click()
        pg.wait_for_selector("text=LIVE", timeout=20000)
        time.sleep(1.0)
        mine = [e for e in api(base, "/api/state?limit=6000")[1]["e"] if e[4] == 1 and e[0] not in before]
        check("signing in over the web gateway adds exactly one player", len(mine) == 1, f"new players: {len(mine)}")
        x0 = mine[0][1]
        pg.keyboard.down("d"); time.sleep(1.2); pg.keyboard.up("d"); time.sleep(0.5)
        now = [e for e in api(base, "/api/state?limit=6000")[1]["e"] if e[0] == mine[0][0]]
        check("holding D walks the player east", bool(now) and now[0][1] - x0 > 5, f"{x0} -> {now[0][1] if now else None}")
        time.sleep(0.4)
        hud = pg.get_by_test_id("hud")
        sx, sy = float(hud.get_attribute("data-self-x")), float(hud.get_attribute("data-self-y"))
        srvpos = [e for e in api(base, "/api/state?limit=6000")[1]["e"] if e[0] == mine[0][0]][0]
        check("the predicted position agrees with the server after stopping", abs(sx - srvpos[1]) < 2 and abs(sy - srvpos[2]) < 2, f"client {sx},{sy} server {srvpos[1]},{srvpos[2]}")
        check("the minimap is on screen", pg.get_by_role("img", name="Minimap of your surroundings").count() == 1)
        # Items: given from the console, shown in the bag, used with Shift+1, dropped and picked up with E.
        mid = mine[0][0]
        check("the console can give an item", api(base, "/api/give", "POST", {"id": mid, "item": "medkit", "count": 2})[0] == 200)
        pg.wait_for_selector('button[aria-label^="medkit ×2"]', timeout=6000)
        check("a given item shows in the bag", True)
        if SHOTS: pg.screenshot(path=str(SHOTS / "play-bag.png"))
        pg.keyboard.press("Shift+Digit1")
        try:
            pg.wait_for_selector('button[aria-label^="medkit ×1"]', timeout=4000)
        except Exception:
            pass
        check("Shift+1 uses the item and one is used up", pg.locator('button[aria-label^="medkit ×1"]').count() == 1)
        api(base, "/api/ground", "POST", {"item": "battery", "count": 1, "x": srvpos[1], "y": srvpos[2]})
        pg.wait_for_selector('button[aria-label^="Pick up 1 battery"]', timeout=6000)
        check("an item at your feet offers a pick-up prompt", True)
        if SHOTS: pg.screenshot(path=str(SHOTS / "play-pickup.png"))
        pg.keyboard.press("e")
        try:
            pg.wait_for_selector('button[aria-label^="battery"]', timeout=4000)
        except Exception:
            pass
        check("E picks it up", pg.locator('button[aria-label^="battery"]').count() == 1)
        inv = api(base, "/api/inventory?id=%d" % mid)[1]["bag"]
        check("the server agrees about the bag", {b["item"]: b["count"] for b in inv} == {"medkit": 1, "battery": 1}, str(inv))
        pg.keyboard.press("g"); pg.keyboard.press("Digit1")
        try:
            pg.wait_for_selector('button[aria-label^="Pick up 1 medkit"]', timeout=4000)
        except Exception:
            pass
        check("G then 1 drops the first slot", pg.locator('button[aria-label^="medkit"]').count() == 0 and pg.locator('button[aria-label^="Pick up 1 medkit"]').count() == 1)
        # Targeting and acting: an idle sentry next to the player.
        game = api(base, "/api/game")[1]
        names = [a["name"] for a in game["archetypes"]]
        st0 = api(base, "/api/state?limit=6000")[1]
        hp_i = 5 + st0["attrs"].index("health")
        known = {e[0] for e in st0["e"]}
        api(base, "/api/spawn", "POST", {"archetype": "sentry", "count": 1, "x": srvpos[1] + 5, "y": srvpos[2]})
        time.sleep(0.5)
        sent = [e for e in api(base, "/api/state?limit=6000")[1]["e"] if e[0] not in known and e[3] == names.index("sentry")]
        check("a sentry was spawned next to the player", len(sent) == 1)
        pg.get_by_role("group", name="Target").wait_for(timeout=4000)
        check("the nearest character is shown as the target", "nearest" in pg.get_by_role("group", name="Target").inner_text())
        pg.keyboard.press("t"); time.sleep(0.3)
        check("T locks the target", "locked" in pg.get_by_role("group", name="Target").inner_text())
        pg.keyboard.press("Escape"); time.sleep(0.3)
        check("Escape releases it", "nearest" in pg.get_by_role("group", name="Target").inner_text())
        pg.keyboard.press("t"); time.sleep(0.2)
        for _ in range(4):
            pg.keyboard.press(" "); time.sleep(0.35)
            if SHOTS and _ == 1: pg.screenshot(path=str(SHOTS / "play-action.png"))
        after = [e for e in api(base, "/api/state?limit=6000")[1]["e"] if sent and e[0] == sent[0][0]]
        check("pressing space hurts the target", bool(after) and after[0][hp_i] < sent[0][hp_i], f"{sent[0][hp_i] if sent else None} -> {after[0][hp_i] if after else 'gone'}")
        pg.get_by_role("button", name="Controls help").click()
        check("the help panel lists the controls", pg.get_by_role("region", name="Controls").inner_text().count("\n") >= 4)
        pg.get_by_role("button", name="Controls help").click()
        snd = pg.get_by_role("button", name="Sound effects"); snd.click()
        check("the sound toggle is a pressed button", snd.get_attribute("aria-pressed") == "true"); snd.click()
        pg.keyboard.press("Enter"); pg.get_by_label("Chat message").fill("hello from the browser"); pg.keyboard.press("Enter"); time.sleep(0.8)
        lines = api(base, "/api/chat")[1]["lines"]
        check("a chat line typed in the browser reaches the server", any(l[3] == "hello from the browser" for l in lines))
        check("the HUD shows the player's own line back", pg.get_by_text("hello from the browser").count() >= 1)
        skin = lambda: pg.evaluate("getComputedStyle(document.querySelector('.play-skin')).getPropertyValue('--hud-bg').trim()")
        was_dark = pg.evaluate("document.documentElement.classList.contains('dark')")
        bg1 = skin()
        pg.get_by_role("button", name="Switch to light theme" if was_dark else "Switch to dark theme").click(); time.sleep(0.8)
        bg2 = skin()
        check("the theme button swaps the play skin", bg1 != bg2 and pg.evaluate("document.documentElement.classList.contains('dark')") != was_dark, f"{bg1} -> {bg2}")
        res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,n:v.nodes.length,t:v.nodes.slice(0,2).map(n=>n.target.join(' '))})))")
        check("play page in game, other theme: no axe violations", not res, json.dumps(res)[:600])
        if SHOTS: pg.screenshot(path=str(SHOTS / ("play-light.png" if was_dark else "play-dark.png")))
        pg.get_by_role("button", name="Switch to dark theme" if was_dark else "Switch to light theme").click(); time.sleep(0.8)
        check("the game keeps running after a theme switch", pg.get_by_test_id("hud").count() == 1 and pg.get_by_text("LIVE").count() >= 1)
        api(base, "/api/weather", "POST", {"name": "fog", "duration_secs": 0}); time.sleep(6.5)
        check("the play page shows the weather the console started", "fog" in pg.get_by_test_id("hud").inner_text().lower())
        if SHOTS: pg.screenshot(path=str(SHOTS / "play-fog.png"))
        api(base, "/api/weather", "POST", {"name": None}); time.sleep(6.5)
        check("and drops it when the console clears it", "weather" not in pg.get_by_test_id("hud").inner_text().lower())
        pg.get_by_role("button", name="Recovery code").click(); time.sleep(0.3)
        import re
        code = pg.locator("code").first.inner_text()
        check("the recovery code has the documented shape", bool(re.fullmatch(r"[0-9a-f]{8}(-[0-9a-f]{8}){7}-[0-9a-f]{4}", code)), code)
        res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,n:v.nodes.length,t:v.nodes.slice(0,2).map(n=>n.target.join(' '))})))")
        check("play page in game: no axe violations", not res, json.dumps(res)[:600])
        check("play page raised no console errors", not perr, str(perr))
        wst = api(base, "/api/state")[1].get("web", {})
        check("the gateway reports the connection", wst.get("open", 0) >= 1, str(wst))
        if SHOTS: pg.screenshot(path=str(SHOTS / "play.png"))
        ctx.close()

        # Phone width: no horizontal scroll.
        ctx = br.new_context(viewport={"width": 390, "height": 800}); pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=Server running"); time.sleep(0.5)
        sw = pg.evaluate("document.documentElement.scrollWidth"); cw = pg.evaluate("document.documentElement.clientWidth")
        check("no horizontal scroll at 390 px", sw <= cw + 1, f"scrollWidth {sw} > {cw}")
        if SHOTS: pg.screenshot(path=str(SHOTS / "phone.png"))
        ctx.close()
        br.close()
finally:
    for p in (bots, srv):
        if p: p.send_signal(signal.SIGTERM)
    time.sleep(0.3)
    shutil.rmtree(work, ignore_errors=True)

failed = [r for r in results if not r[1]]
print(f"\n{len(results) - len(failed)}/{len(results)} UI checks passed")
sys.exit(1 if failed else 0)

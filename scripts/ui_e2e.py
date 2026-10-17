#!/usr/bin/env python3
"""Drive the built UI in a real browser against a real server and check it works.

Usage: scripts/ui_e2e.py [--shots DIR]
Needs: cargo build --release, (cd ui && npm run build), python3 playwright + chromium.
Starts its own server on free ports with a throwaway game.json and key.
"""
import json, os, shutil, signal, socket, subprocess, sys, tempfile, time, urllib.request
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
udp, http = free_port(True), free_port()
base = f"http://127.0.0.1:{http}"
srv = subprocess.Popen([str(SERVER), "--port", str(udp), "--admin", f"127.0.0.1:{http}", "--ui-dir", str(ROOT / "ui/dist"),
                        "--world-size", "1024", "--quiet", "--timeout", "30"], cwd=work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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

    with sync_playwright() as pw:
        br = pw.chromium.launch(args=["--no-sandbox"])
        TABS = ["Bots", "Attributes", "Actions", "Entity", "Connect", "JSON"]
        for theme in ("dark", "light"):
            ctx = br.new_context(viewport={"width": 1440, "height": 900}, color_scheme=theme)
            pg = ctx.new_page()
            errors = []
            pg.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            pg.on("pageerror", lambda e: errors.append(str(e)))
            pg.goto(base + "/")
            pg.wait_for_selector("text=LIVE", timeout=8000)
            check(f"[{theme}] page loads live with no console errors", not errors, str(errors))
            check(f"[{theme}] theme follows the system preference", ("dark" in (pg.evaluate("document.documentElement.className"))) == (theme == "dark"))
            for t in TABS:
                pg.get_by_role("tab", name=t, exact=True).click()
                time.sleep(0.3)
                pg.add_script_tag(path=str(AXE)) if not pg.evaluate("!!window.axe") else None
                res = pg.evaluate("axe.run(document, {runOnly:['wcag2a','wcag2aa','wcag21aa','best-practice']}).then(r=>r.violations.map(v=>({id:v.id,impact:v.impact,n:v.nodes.length,t:v.nodes.slice(0,3).map(n=>n.target.join(' ')+' :: '+(n.failureSummary||'').split('\\n').slice(0,3).join(' | '))})))")
                check(f"[{theme}] accessibility, {t} tab: no axe violations", not res, json.dumps(res)[:900])
                if SHOTS: pg.screenshot(path=str(SHOTS / f"{theme}-{t.lower()}.png"))
            ctx.close()

        # --- behaviour tests (dark theme, desktop)
        ctx = br.new_context(viewport={"width": 1440, "height": 900}, color_scheme="dark")
        pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=LIVE")
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
        pg.get_by_role("tab", name="Bots", exact=True).click()
        before = api(base, "/api/state")[1]
        cnt = pg.get_by_label("Spawn count").first
        cnt.fill("20")
        check("Apply button enables when the draft differs", pg.get_by_role("button", name="Apply", exact=True).is_enabled())
        check("bot-only change says it applies live", pg.locator("text=Bot changes only").count() == 1)
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

        # Possession with the keyboard.
        pg.get_by_role("tab", name="Entity", exact=True).click()
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
        pg.get_by_role("tab", name="Attributes", exact=True).click()
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
        pg.get_by_role("tab", name="JSON", exact=True).click()
        ta = pg.locator("textarea")
        ta.fill("{ not json")
        check("invalid JSON shows an error", pg.locator("[role=alert]").count() >= 1)
        # Rename propagation through the form.
        pg.get_by_role("tab", name="Attributes", exact=True).click()
        pg.get_by_role("tab", name="JSON", exact=True).click()
        txt = ta.input_value()
        check("draft is restored from the last good state", '"attrs"' in txt)

        # Theme persistence.
        pg.get_by_role("button", name="Switch to light theme").click()
        pg.reload(); pg.wait_for_selector("text=LIVE")
        check("theme choice survives a reload", "dark" not in pg.evaluate("document.documentElement.className"))
        ctx.close()

        # Keyboard-only: arrow keys move between tabs (Radix roving focus).
        ctx = br.new_context(viewport={"width": 1440, "height": 900}); pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=LIVE")
        pg.get_by_role("tab", name="Bots", exact=True).focus(); pg.keyboard.press("ArrowRight"); pg.wait_for_timeout(300)
        check("arrow keys switch tabs", pg.get_by_role("tab", name="Attributes", exact=True).get_attribute("data-state") == "active")
        ctx.close()

        # Phone width: no horizontal scroll.
        ctx = br.new_context(viewport={"width": 390, "height": 800}); pg = ctx.new_page(); pg.goto(base + "/"); pg.wait_for_selector("text=LIVE"); time.sleep(0.5)
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

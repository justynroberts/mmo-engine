# God

A live control desk for an mmo-engine world, as a desktop app: the map with everyone on it, the time of day and the
weather, spawning at a click, world events, and the server's own voice (a banner or a chat line to every player).

It talks only to the server's admin interface (`/api/*`, see "Live console" in the repository's `CLAUDE.md`), which
on the hosted server listens on the Tailscale address only: be on the tailnet first.

```sh
cd god
npm install
npm start            # opens on the connect form: server address and the admin password
npm test             # the world-file reduction
npm run dist         # dist/mac-arm64/God.app (unsigned)
```

The admin password is typed once and kept encrypted by the macOS keychain (Electron `safeStorage`) in the app's own
data folder; the window never holds it. The main process makes every request and will only call `/api/` on the one
server it was pointed at.

From a terminal, without saving anything: `GOD_SERVER=http://host:7791 GOD_PASSWORD=... npm start`.
`GOD_DRY=1` refuses every write before it leaves the machine (look, do not touch). `GOD_SHOT=file.png` draws the
desk, saves a picture and quits.

## What each strip does

| Strip | Server call |
|---|---|
| Sky: time of day, weather for a while or until changed, weather zones | `POST /api/daylight`, `/api/weather`, `/api/zone` |
| Spawn: a creature type, a count and a scatter; arm, then click the map (or Enter for the centre) | `POST /api/spawn` |
| Kill all bots (press twice) | `POST /api/kill` |
| Voice: a banner or a chat line to every player | `POST /api/announce` |
| Events: fire or stop | `POST /api/events/fire`, `/api/events/stop` |
| Selected: follow, god mode, kill, give an item, speak as it | `POST /api/god`, `/api/kill`, `/api/give`, `/api/say` |

The map is the world file cut down in the main process (`world.js`) to a 768-cell picture of the land, the roads,
rivers and settlements; creatures come from `/api/state` every 2.5 s (up to 20,000 rows).

# Console design

**Chrome: Admin console, sections that say what is inside them.** Near-black, one proportional face (Instrument Sans,
variable) at 12/14/15/20px, sentence-case labels, boxed inputs and 32px buttons. One accent, orange
(`--color-accent`), kept for live state, the selected section, switches that are on and the primary action. No cards
or shadows; hairlines and a 2-3% lift (`--color-bg-2`, `--color-bg-3`) separate things; radius 4px.

The recognisable idea, taken from teenage engineering's navigation: the top-level tabs are an index. Each shows an
icon, a name and what is inside ("Sky: day, weather, zones"), so the console is learnable without opening anything.
Sections are by task, not by the game file's shape: Live, World, Bots, Sky, Events, Rules, Server. Every page opens
with a title and one plain sentence of help, then its sub-pages as a quiet underline tab row. Apply / Revert live in
the header, because changes made on any page go out together.

Replaced (2026-10-19) the earlier Instrument-panel chrome (Martian Mono, uppercase labels, underline inputs, eleven
flat tabs in a side panel), which read as a terminal. Its tick-budget ruler stays, on the Server page.

**Map: the game's world, not the page's.** The 2D map and 3D view use the same palette as the Godot client
(`src/components/mapLook.ts`): meadow ground with seeded hill shading, forest past the edge, stone buildings, trees
from the world seed, gold for selection, ink rims on every marker and label so they read on grass. It is content, like
a photo in a page, so it does not follow the chrome's one-accent rule. It is docked beside the pages that work on it
(Live, World, Bots) and absent from the rest, which get a readable 900px column.

The play page keeps its own look (`.play-skin`, Martian Mono HUD); the console's styles are scoped to `.console`.

Motion only on state change (150ms ease-out), `prefers-reduced-motion` honoured. Info button ("Made by FintonLabs")
in the header.

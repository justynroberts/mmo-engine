# Console design

**Chrome: Instrument panel.** Hardware-UI logic on near-black: small-caps labels, values on a visible
grid, inputs as underlines, hairline rules instead of boxes. One typeface, Martian Mono, at 11/13/18px.
One accent, orange (`--color-accent`), kept for live state, selection and the primary action.
The recognisable idea: the tick budget drawn as a ruler, with the p99 as its marker.

**Map: the game's world, not the page's.** The 2D map and 3D view use the same palette as the Godot
client (`src/components/mapLook.ts`): meadow ground with seeded hill shading, forest past the edge,
stone buildings, trees from the world seed, gold for selection, ink rims on every marker and label so
they read on grass. It is content, like a photo in a page, so it does not follow the page theme and
the chrome's one-accent rule does not apply inside it.

Motion only on state change (150ms ease-out), `prefers-reduced-motion` honoured. Info button
("Made by FintonLabs") in the header.

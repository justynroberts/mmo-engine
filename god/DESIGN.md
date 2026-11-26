# Design

**Direction:** instrument panel (`distinctive-ui`). A mixing desk for a world: the map is the instrument's one
display, the right-hand rack is channel strips of keys.

**Typeface:** B612 Mono only (the cockpit face Airbus commissioned for flight-deck displays), 400 and 700,
self-hosted from `@fontsource/b612-mono`. Three sizes: 11 (labels and keys, uppercase, tracked), 13 (everything),
20 (the one readout per strip).

**Colour:** near-black `#0b0c0e`, a four-step grey ink ramp, and ONE accent, amber `#f2a900`. Amber means "this is
on, or armed, or a player": the lit key, the armed line under the map, players on the map, the link when it is lost.
Nothing else is ever amber. Creatures are grey; land is a short grey ramp; the sea is the panel itself.

**Grid:** 8px. Map and tape on the left take what is left; the rack is a fixed 400px on the right. No cards, no
shadows, radius 2px, sections parted by 1px hairlines.

**The recognisable idea:** one kind of button, the key, and it lights only while its thing is live. Arming the
spawn lights the key, the crosshair and a line under the map, and the next click on the map fires.

**Motion:** a key's border and text change colour over 140ms; nothing else moves. `prefers-reduced-motion` honoured.

**References** (screenshots in `design/moodboard/`): Teenage Engineering TX-6 and OP-1 (labelled knobs on black,
one small display), Elektron Digitakt (numbered pads that light), Ableton Push, monome norns, QLab's cue list,
Foundry VTT and a flight-tracking map (what a live map of many moving things needs). Common to the hardware:
every control labelled in small caps, light only where something is active.

**Info button:** the `i` key at the end of the top strip: name, version, Made by FintonLabs, forget password.

## UX review (2026-11-24)

Tasks walked: change the sky during play; spawn creatures by a player; say something to everybody. States forced:
first run (connect form), a refused connection, the desk at 1440x900 and at its minimum 1100x700, a player
selected, the link lost. All captures were dry runs (`GOD_DRY=1`), so no write was sent.

Fixed: lists were rebuilt on every poll, which lost a click that straddled one (they are now rebuilt only when
their rows change, and countdowns and positions are written in place); a double click sent a control twice;
god mode could be turned on but not off; a player died on one press; the window was blank while a saved password
was tried; a refused connection cleared what was typed; each arrow key on the time slider was its own request;
weather keys carried numbers that were not shortcuts; the top strip could push the info key off a narrow window.

Closed 2026-11-26: with the map focused, Enter selects the creature nearest the middle and `]` / `[` step to the
next and the one before, among those in view; a refusal breaks the outline of the control that sent it (dashed,
ink, never the accent) and puts the reason in its tooltip, until the next thing is sent; each section of the rack
folds to its heading (kept for next time) and the events list shows seven and scrolls, so on a short window Voice
or Selected can be brought up beside the map.

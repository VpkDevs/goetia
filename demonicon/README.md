# DEMONICON

A stylish occult ARPG vertical slice on the GOETIA engine (Shot 2 of 2).
The 72 demons of the Ars Goetia are civilizations; three seats are lit —
VASSAGO (hidden things), ANDRAS (discord), BUER (the healer-wheel). Every
mechanic composes with every other mechanic, and finding something hilariously
broken is the point of the game.

## Run

```bash
cd goetia
cargo run -p demonicon --release
```

Debug flags: `--seed N` (reproduce a session) · `--demon 0..2 --tier N`
(skip the Court, descend immediately) · `--novsync` · `--frames N` (bench+exit).
Saves live in `demonicon/saves/slot0.ron` (bank, build, progress — atomic writes).

## Controls

| Input | Action |
|---|---|
| WASD | Move |
| Mouse | Aim |
| LMB / RMB / Q / E / R / F | Six skill slots |
| Space | Dodge (i-frames) |
| G | Interact — portals, shrines, the corruption altar |
| Tab | Spoils: inventory, equipment, rites, sigils, pacts |
| 1–3, +/-, Enter | Court: pick demon, tier, descend |
| F1 | Engine debug overlay |
| Esc | Close menu / save & quit |

## The loop

Court → pick a seat and a tier → 8–15 min procedural realm → boss → bank loot
at a portal → rebuild at Court (respec is free, always) → higher tier.
Tiers are infinite; tier 2+ realms roll 2–4 modifiers that scale danger *and*
loot. Death forfeits unbanked spoils only — you're back in a run in seconds.

## What composes

- **4 damage types** (Physical/Hellfire/Hex/Void) · **6 statuses** (ignite
  spreads on death, hexmark amplifies-and-consumes, discord turns enemies on
  each other, blight detonates at cap, petrify shatters under physical,
  consecrate flips heal/harm) · **8 triggers** (kill, crit, status-apply,
  status-detonate, every-5th-cast, dodge, loot-pickup, low-life).
- **8 skills × 3 sigil sockets** (20 sigils: conversions, echoes, orbits,
  behavioral reactions) · **9 contracts** (max 3, real drawbacks) ·
  **~65 affixes** (15+ behavioral) · **12 hand-designed Goetics** ·
  a **corruption altar** that can brick, rewrite, mark — or **awaken** an item
  into a procedurally named Goetic.
- There are no "X can't proc Y" rules anywhere. Loops are damped by the
  engine's trigger budget, never forbidden.

All content is RON under `demonicon/data/` and hot-reloads in debug builds.

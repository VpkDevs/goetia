---
description: Tune DEMONICON survivability/pacing via RON data, then re-verify. Balances by adding power, never by adding exclusions.
argument-hint: "[what feels off] — e.g. 'tier 1 kills me too fast' or 'boss is a HP sponge'"
---

Balance concern: $ARGUMENTS

The DEMONICON balance philosophy is Pillar 1: fun outranks balance. You tune to
make the game *feel right to play*, never to close off broken builds. If a combo
is overpowered, that stays — you lift the floor around it, you don't cap it.

Levers, in preference order (data before code):
- Enemy `hp`/`dmg`/`speed` and `weight` in `demonicon/data/enemies.ron`.
- Realm-mod multipliers in `realm_mods.ron` (must move danger AND loot together).
- Tier scaling constants in `demonicon/src/enemies.rs` (`tier_hp`, `tier_dmg`).
- Player baselines (`K_MAX_HP`, `K_REGEN`) and grace frames in
  `combat.rs`/`items.rs` — last resort, they touch every build.

Known-good reference points already tuned: base 170 HP, ~28 post-hit grace
frames, pack density `2 + tier/2`. The first playtester died AFK in 5s before
these; don't regress below them.

After any change:
`$env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"; cargo test -p demonicon`
then spot-check feel with `/playtest`. Determinism tests must stay green (tuning
is data, so they should — if they redden, you changed sim logic, not numbers).

Report what you moved, by how much, and why the new value is right.

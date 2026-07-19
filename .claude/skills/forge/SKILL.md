---
name: forge
description: Add or modify DEMONICON game content — skills, sigils, affixes, contracts, Goetics, enemies, realms, realm-mods, statuses. Use whenever the task is "add a <thing>", "make a <mechanic>", "buff/nerf", "new demon content", or editing anything under demonicon/data/*.ron. Everything the game does lives in RON on one shared vocabulary; this skill is how you extend it without breaking Pillar 2.
---

# Forge — DEMONICON content authoring

**Everything gameplay-defining is RON data under `demonicon/data/`.** Rust is the
*mechanism*; RON is the *content*. If you find yourself adding a Rust field to
express one skill's behavior, stop — that's Pillar 2 violated. The behavior
almost certainly composes out of the existing vocabulary below.

## The one rule that outranks the others

**Pillar 1 — fun outranks balance. Never add a "X can't proc Y" exception.**
Loops are content; the engine's trigger budget is the only referee. If a
combination is degenerate, that is a *feature to leave in*, not a bug to gate.
Balance by making more things overpowered, never by adding exclusions.

## Build discipline (this machine)

MinGW must be on PATH or every `cargo` invocation fails at link:

```powershell
$env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"
```

Never edit anything under `crates/` — Shot 1 (the `goetia` engine) is **frozen**.
Game code depends only on the `goetia` facade. If the contract genuinely blocks
you, log it in `demonicon/BUILD_NOTES.md` and route around it.

## The shared vocabulary (all of it)

Reference these exact strings/idents — they are FNV-keyed, so a typo silently
becomes a different (dead) key rather than a compile error.

- **Damage types** (`DmgType`): `Physical` `Hellfire` `Hex` `Void`
- **Statuses** (string ids): `ignite` (spreads on death) · `hexmark` (amplifies
  & is consumed by the next non-physical hit) · `discord` (retargets enemies at
  each other) · `blight` (detonates at cap) · `petrify` (shatters under physical)
  · `consecrate` (flips heal/harm on the ground)
- **Triggers** (`on:` strings): `on_kill` `on_crit` `on_status_apply`
  `on_status_detonate` `nth_cast` `on_dodge` `on_loot_pickup` `on_low_life`
- **Stat keys** (affix/contract/Goetic `stat:` strings): `max_hp` `hp_regen`
  `move_speed` `cast_speed` `crit_chance` `crit_mult` `dmg_global` `dmg_phys`
  `dmg_hellfire` `dmg_hex` `dmg_void` `status_chance` `aoe` `proj_speed` `armor`
  `resist` `loot_quant` `loot_rare` `minion_dmg`
- **Slots**: `Weapon` `Armor` `Relic` `Ring`

### The universal behavioral unit: `Reaction`

Sigils, affixes, contracts, and Goetics ALL carry these. One processor runs them.

```ron
(on: "on_kill", chance: 0.35, action: <Action>)   // chance defaults to 1.0
```

`Action` variants (exhaustive):
- `Nova(pct: 0.8, dtype: Hellfire, radius: 3.5)` — % of weapon power at the target
- `ApplyStatus(status: "ignite", stacks: 2, magnitude: 0.5, radius: 0.0)`
- `Echo(pct: 1.0)` — recast last skill at pct power (echoes count as casts → loops)
- `FreeReset` — zero the last skill's cooldown
- `Heal(pct_max: 0.25)`
- `SpreadStatus(status: "ignite", radius: 5.0)` — copy target's stacks to neighbors
- `Detonate(status: "blight")`
- `Frenzy(ticks: 240, cast_speed: 0.4, move_speed: 0.25)`
- `Dust(amount: 1)`

### Rule (rule-changers, on contracts & Goetics)

`AllHiddenActive` `PlayerDiscordable(power: 0.8)` `ProcsTargetSelf` `LockBlight`
`BlightHealsYou` `AppraiseOnPickup` `DoubleDamageDelayed` `AllHellfire`
`EternalIgnite` `BloodDodge` `CritsPetrify` `StillnessConsecrates`
`ServantsInherit` `LootGravity`. Adding a *new* Rule variant is the one time you
touch Rust: add to `content.rs::Rule`, then read it wherever it applies (grep an
existing variant like `AllHellfire` to see the full wiring — enum, `rule_line`
display, and the consuming site).

## Which file, which type

| Want to add… | File | Struct | Notes |
|---|---|---|---|
| a verb (the 6 slots) | `skills.ron` | `SkillDef` | `kind`: Projectile/Nova/Ground/Minion/Beam/Dash/Curse/Totem |
| a socketed modifier | `sigils.ron` | `SigilDef` | `ops`: Echo/Convert/Pierce/Orbit/AreaMul/CdMul/DmgMul/SpeedMul/CountAdd/DurationMul/AddApply/React |
| an item mod | `affixes.ron` | `AffixDef` | numeric (stat+op+lo+hi) OR behavioral (reaction). `hidden_pool`/`curse`/`corrupt_only` flags |
| a build-defining pact | `contracts.ron` | `ContractDef` | `demon` + real drawback. Max 3 active in-game |
| a unique | `goetics.ron` | `GoeticDef` | mods + reactions + rules. Honor the 50/25/15/10 mix |
| an enemy | `enemies.ron` | `EnemyDef` | `shape` + `ai` carry identity — no rigs exist |
| a realm | `realms.ron` + `realms/*.ron` | `RealmDef` + templates/grammar | |
| a run modifier | `realm_mods.ron` | `RealmModDef` | must scale danger AND loot |
| a status | `statuses.ron` | `StatusDef` | engine machinery; meaning is in `combat.rs` |

## The forge checklist

1. **Read the neighbor.** Open the target `.ron`, copy the closest existing
   entry, mutate it. The files are self-documenting by example.
2. **Reuse the vocabulary.** New verb? Pick an existing `SkillKind`. New
   behavior? It's a `Reaction`. Only invent a Rust variant for a genuinely new
   *rule-changer* (see above).
3. **IDs are lowercase snake, unique per file.** `content.rs::validate` checks
   duplicates and dangling enemy/boss references.
4. **Verify:**
   ```powershell
   $env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"
   cargo test -p demonicon --test acceptance
   ```
   `content_loads_and_validates` catches RON parse errors, missing refs, and the
   count invariants (8 skills, 20 sigils, 9 contracts, 12 Goetics, ≥55 affixes).
   `every_contract_goetic_sigil_combination_compiles` proves your addition
   composes with all others.
5. **In debug builds RON hot-reloads** — `cargo run -p demonicon` then edit and
   watch it change live (the game polls once/sec).

## When you add something deliberately broken

That's the job (Pillar 1). Seed it undocumented into the data, then record the
interaction in `demonicon/BUILD_NOTES.md` under "seeded broken builds". Do not
add a guard. Do not nerf it. The dopamine event is the player finding it.

# BUILD_NOTES — DEMONICON (Shot 2)

One page. Decisions, the seeded degenerate builds, contract gaps, cuts.

## Decisions

- **One reaction vocabulary.** Sigils, affixes, contracts and Goetics all carry
  the same `Reaction { on, chance, action }` struct, executed by one processor
  off the engine trigger bus. Pillar 2 is a data type, not a guideline.
- **Engine untouched.** The game builds against the `goetia` facade only; zero
  engine-internal edits were needed in this shot.
- **All 20 sigils, 8 skills, 9 contracts available from minute one.** Loot
  drops are items only. Rationale: sigil/skill acquisition plumbing would have
  eaten the feel budget, and the experimentation surface *is* the game. Free
  respec extends to "free everything-but-items".
- **Damage pipeline is single-path.** Every source — skill, DoT, minion, totem,
  echo, altar-born horror, discord riot — goes through `hit_enemy`; enemy →
  player through `hit_player`. Rule-changers (AllHellfire, DoubleDamageDelayed,
  CritsPetrify…) are flags read inside that one path.
- **Menu pauses the run.** Solo game, honest pause.
- **Synthesized audio** (engine synth API): zero binary assets in the repo.

## The three seeded broken builds (DO NOT FIX — Pillar 1)

1. **The Ignite Chain Reactor.** Sigil of the Detonator (`on_status_detonate:
   recast free`) + "15%: APPLYING A STATUS DETONATES BLIGHT" (`b_popblight`) +
   Sigil of the Rot on a fast skill. Applies detonate their own fuel, detonations
   refund the cast, kills spread ignite twice with `b_spread`. Sustained, the
   proc engine readout pins at the trigger budget — which is the win condition.
2. **The Discord Martyr.** Andras' CONTRACT OF DISCORD (+80% damage while
   Discorded, procs may target you) + "ON LOW LIFE: HEAL 25%" + the Consecrate
   ritual circle (incoming damage on consecrated ground heals half instead).
   Your own misfiring procs feed the low-life trigger, the circle converts the
   punishment into sustain: a glass cannon that drinks its own shrapnel.
3. **The Counting Loop.** "EVERY 5TH CAST ECHOES AT FULL POWER" (`b_fifthecho`)
   + Sigil of the Choir (echo 65%) + MOUTH THAT COUNTS (nth-cast hex nova).
   Echoes count as casts, so echoes advance the counter that spawns more
   echoes. Beam ticks count too. The cast counter becomes a flywheel.

All three emerge purely from RON data. Undocumented in-game.

## API-contract gaps found (Shot 1 defects, routed around per the rules)

- **No `Events<T>` bridging for StatusBag inside `world.each`** — borrow
  discipline forces the take-the-bus-out / clone-the-stream-out patterns the
  contract itself documents. Workable, but a `Engine::with_triggers(|bus, world|)`
  helper would remove the boilerplate.
- **`TriggerEmitter` can't be threaded through deferred action execution**, so
  reaction *consequences* re-enter as root emissions: depth/falloff accounting
  restarts per hop. The global budget still contains everything (verified by
  test), but chain-depth stats under-report. Engine-side fix would be an
  `emit_at_depth` API.
- **UI layer has no clip/scissor**, so long item lists are windowed manually.

## Deviations & cuts (per scope-cut order)

- Nothing from the never-cut list was cut. All three realms shipped.
- **Corruption altar targets the most recent pickup** rather than a picker UI
  (the altar UI would have raided the feel budget; the bench at Court gives
  targeted crafting).
- **Hidden-affix reveal** is Vassago shrines + Appraisal contract + the
  `unveiled` realm mod. No per-item reveal ritual.
- **Boss arenas are rooms, not set-pieces.** Phases, adds, reflections and
  wheel-immunity shipped; bespoke arena geometry did not.
- Tuning pass after first playtest: +50 base HP, +grace frames on hit,
  enemy damage −25–35%, tap-casting fixed (edge OR hold). The first AFK
  player died in 5 seconds; the second survived to fight. Working as intended
  now — death #1 in testing was "why is the wraith pack eating me", which is
  criterion 4.

## Verification

- `cargo test -p demonicon`: content validation, all
  goetic×contract×sigil build compilations, loot/pity, corruption awakening,
  trigger-loop containment under a self-feeding reaction table, and
  2000-tick full-run determinism per realm (headless).
- Playtested on-machine: court → descend → combat with damage numbers,
  telegraphs, loot beams, banking, death flow. 60 fps, 0 frames >20 ms
  during the fight scene (engine overlay).

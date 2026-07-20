# AGENTS.md — GOETIA / DEMONICON

Operating manual for any agent working in this repo. Read this before acting.

## What this is

Two shipped artifacts in one Cargo workspace:

- **GOETIA** (`crates/`, `sandbox/`) — a bespoke, desktop-native, data-oriented
  Rust game engine. **Frozen.** Its public surface is `API_CONTRACT.md`.
- **DEMONICON** (`demonicon/`) — an occult ARPG vertical slice built *on* that
  engine, depending only on the `goetia` facade. All its content is RON under
  `demonicon/data/`.

## Non-negotiable invariants

1. **The engine is frozen.** Never edit anything under `crates/`. Game code
   imports only from `goetia` (see `goetia::prelude`). A genuine contract gap
   gets logged in `demonicon/BUILD_NOTES.md`, never patched by forking the
   engine mid-work.
2. **Everything is RON vocabulary** (Pillar 2). Skills, sigils, affixes,
   contracts, Goetics, enemies, realms are *data* on one shared vocabulary
   (4 damage types, 6 statuses, 8 triggers, one `Reaction` unit). Adding a Rust
   field to express one item's behavior is the wrong move — it almost always
   composes from existing pieces.
3. **Fun outranks balance** (Pillar 1). Never add a "X can't proc Y" exception.
   Degenerate combos are content; the trigger budget is the only referee.
   Balance by adding power, not exclusions.
4. **Determinism is load-bearing.** The sim is fixed-tick 60Hz and
   bit-reproducible. No wall-clock or un-seeded randomness in `fixed_update`;
   all sim RNG flows through named `eng.streams` PCG streams. The determinism
   tests are the tripwire.
5. **The feel budget is sacred** (Pillar 5). ~25% of effort is game feel and it
   is never raided for features.

## Build & test (this machine)

MinGW must be on PATH first, or every `cargo` command fails at the linker
(no MSVC `link.exe` here — the workspace pins the `windows-gnu` toolchain):

```powershell
$env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"
cargo test --workspace                 # engine + game, incl. determinism
cargo run  -p demonicon --release      # play it
cargo run  -p sandbox   --release      # engine stress/feel proof
```

Debug launch flags: `--demon 0..2 --tier N --seed N` (skip Court, descend
straight in) · `--frames N` (bench + exit) · `--novsync`.

## The toolkit in `.claude/`

- **`skills/forge/SKILL.md`** — the content-authoring skill: full vocabulary,
  file→struct table, verify checklist. Invoke it for any "add a <thing>" task.
- **`agents/content-smith.md`** — RON content designer.
- **`agents/engine-warden.md`** — enforces the frozen-engine wall & determinism;
  classifies "engine can't do X" as misuse / routable gap / true defect.
- **`agents/feel-critic.md`** — playtests headlessly and windowed; judges juice.
- **`commands/`** — `/forge`, `/playtest`, `/balance` prompt workflows.

## Verify before you claim

"It compiles" is not "it works." The game has a headless acceptance suite
(`cargo test -p demonicon`, 8 tests incl. the full boss-kill→clear→loot loop and
per-realm determinism) and a windowed feel check. Use both; state honest
failures plainly.

## Version control & repo health

`DEV/goetia` is a git repo; remote is `VpkDevs/goetia` (private). `DEV` itself is
a ~400-repo workspace and is deliberately NOT a repo — never `git init` at `DEV`
or the profile root. Confirm `git rev-parse --show-toplevel` prints
`.../DEV/goetia` before committing.

**Enable the pre-push gate once per clone:**
```bash
git config core.hooksPath .githooks
```
It runs fmt + clippy(`-D warnings`) + the full test suite before every push.
`git push --no-verify` bypasses it for WIP branches.

**Quality bar (all currently green, keep them that way):**
`cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings`
· `cargo test --workspace` (39 tests). Line endings are normalized to LF by
`.gitattributes`; formatting is `rustfmt.toml` (max_width 100).

**⚠️ GitHub Actions is currently blocked at the account level.** Every workflow —
including a 9-line hello-world and Dependabot's own managed run — fails with
`startup_failure` in ~1s. This is NOT a workflow-file bug (`.github/workflows/ci.yml`
is valid and all four of its gates pass locally). Private repos consume Actions
minutes; the fix is on the billing/quota side, or make the repo public for
unlimited free minutes. Until then the pre-push hook is the enforcement point.

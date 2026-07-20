## What & why

<!-- One or two sentences. What changes, and what it buys. -->

## Which layer

- [ ] **Game content** (`demonicon/data/*.ron`) — data only
- [ ] **Game code** (`demonicon/src/`)
- [ ] **Engine** (`crates/`) — ⚠️ Shot 1 is frozen; justify below or reconsider
- [ ] Tooling / CI / docs

## Invariants

- [ ] No `"X can't proc Y"` exclusion was added (Pillar 1 — degenerate combos stay)
- [ ] New behavior composes from the shared vocabulary (Pillar 2), not a private flag
- [ ] Nothing in `fixed_update` reads wall-clock or un-seeded randomness (determinism)
- [ ] Engine untouched, or the reason it had to change is stated here

## Verification

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- [ ] Above are green locally
- [ ] If gameplay changed: actually played it (`cargo run -p demonicon --release`),
      and said what it felt like — not just that it compiled

## Notes for review

<!-- Seeded-broken-build? Contract gap found? Tuning rationale? -->

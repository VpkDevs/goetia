---
description: Forge new DEMONICON content (skill / sigil / affix / contract / Goetic / enemy / realm-mod) as RON on the shared vocabulary.
argument-hint: "<what to add> — e.g. 'a sigil that converts crits into ignite stacks'"
---

Forge this content: $ARGUMENTS

Invoke the `forge` skill first — it holds the full vocabulary and the file→struct
table. Then:

- Express it as data in the right `demonicon/data/*.ron` file, copying the
  nearest existing entry as a template.
- Reuse the vocabulary: a new behavior is a `Reaction { on, chance, action }`,
  not a new code path. Only add a Rust `Rule` variant for a genuinely new
  rule-changer, and wire it end-to-end (grep an existing variant).
- NEVER add a "X can't proc Y" guard. Degenerate is good (Pillar 1). Frozen
  engine — never touch `crates/`.
- Verify:
  `$env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"; cargo test -p demonicon --test acceptance`
- If you seeded something broken on purpose, log it in `demonicon/BUILD_NOTES.md`
  and leave it un-nerfed.

Report the exact IDs added, the vocabulary reused, and the test result.

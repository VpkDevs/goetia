---
description: Verify DEMONICON actually plays — headless suite, then launch-and-drive the real game and judge it against the playtester criteria.
argument-hint: "[demon 0-2] [tier N] — optional; defaults to Vassago tier 1"
---

Playtest DEMONICON. Do NOT declare anything working on a compile alone.

Target: $ARGUMENTS (if empty, Vassago / tier 1).

1. Headless gate first:
   `$env:PATH = "$HOME\scoop\apps\mingw\current\bin;$env:PATH"; cargo test -p demonicon`
   All eight acceptance tests must be green before touching the window.

2. Build release and drive the real game:
   `cargo build -p demonicon --release`, launch
   `target\release\demonicon.exe --demon <N> --tier <N> --seed 31337`
   `-WindowStyle Hidden`, focus it by window title "DEMONICON" (FindWindowA +
   SetForegroundWindow), then synthesize click-fire in an arc around screen
   center for ~8s and screenshot mid-combat with Graphics.CopyFromScreen. Start
   clicking only AFTER the window has focus (launch takes ~1.6s; input before
   that = AFK death).

3. Read the screenshot and judge against the 20-minute-playtester criteria:
   did a screen-clear read as satisfying feel; are damage numbers, telegraphs,
   and loot beams legible; is health readable; 60fps with 0 frames >20ms in the
   overlay. Report what you actually saw, the screenshot path, which criteria
   held, and any feel or legibility regression as the top fix.

Prefer the `feel-critic` agent's discipline throughout: running > claiming.

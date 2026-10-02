# SpaceView gauntlet

Run from the checkout:

```powershell
./tools/gauntlet.ps1 -LivePath 'C:/Github'
```

The command runs Rust regressions, builds the release EXE, captures 18 native UI
states at 1024x700 and 1400x860 logical sizes, probes live scanning for five seconds,
captures a real scan progressing, pausing, resuming and canceling, then runs the
existing 500,000-file camera stress harness. Output includes logs, timing CSVs,
PNG captures and an EXE hash in `test-results/<timestamp>/`.
It also compares cached scans against fresh inventories through real NTFS
mutations, hardlinks, open writers, permission changes and fallback cases. Two
additional native captures exercise Rescan, followed by a cross-process reuse
probe. See `SCAN-CACHE.md` for the validation rules.

Use a large folder for `LivePath`, one that takes at least several seconds to scan.
The probe and live captures read that folder and cancel their own scan. Omitting
`LivePath` runs the core, visual and stress gates. Screenshot fixtures exercise
the actual native egui renderer; pointer/button states are injected into this
app's egui input only. They never generate Windows desktop input.

Test preferences are isolated with `SPACEVIEW_PREFS_DIR`. The harness does not
replace the user's saved theme, window position or About preferences. Automated
capture does not count as visual acceptance: inspect the PNGs and record the
review separately. `-SkipBuild` is for a deliberate repeat with an already built
EXE; normal runs always build.

Scanner regressions cover early publication inside a single deep folder, bounded
preview size, correct final totals, a full preview queue, zero-byte files, empty
directories, case-insensitive system-folder exclusions, pause/resume, cancellation
while paused, and aggregate area/path safety. Contrast sweeps cover 4,096 RGB
colors in each mode, composited gradient/frost settings and real widget visuals.
Text-fitting regressions cover wide glyphs, Unicode filenames, complete numeric
values, insufficient row height and label plates staying within their tile.
The native paused capture must show Paused with a static indicator; the resumed
capture must restore Scanning and its spinner.

The stronger text pass follows Trontop's separation of raw palette intent,
protected text surfaces and corrected foreground ink. Text-bearing chrome uses
a 7:1 ratio floor plus the existing APCA small-text floor; outlines use 3:1.
Treemap labels have protected tinted faces, leaving the rest of each tile vivid.
These are checks of rendered color pairs, not a claim about every antialiased
glyph or platform accessibility certification.

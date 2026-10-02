# SpaceView v0.16.0 verified scan-cache validation

```powershell
./tools/gauntlet.ps1 -LivePath C:/Github -Output C:/trontstack/spaceview/test-results/20261001-cache-ship-v0.16.0
```

Result: `[SpaceViewGauntlet] COMPLETE all automated checks passed`.

- 51 Rust regressions passed. Coverage includes journal versions/continuity,
  permission scope, hardlink routing and compact Unicode path reconstruction.
- 22 native cache cases passed against fresh inventories, comparing paths,
  types, sizes, counts and timestamps. Cases include create/resize/delete/move,
  hardlinks, an external alias, repeated data writes and timestamp-only writes
  through open handles, real ACL changes, replaced roots, corrupt data/checksums,
  journal gaps/resets, changed security context and paused cancellation.
- The 2,001-file baseline took 89.92 ms; its unchanged rescan took 15.80 ms,
  enumerating zero folders and checking 2,001 current file metadata records.
  Reuse from a fresh process took 17.34 ms for the final 2,003-file fixture.
  These are fixture timings, not a whole-drive performance guarantee. The cache
  stores an acceleration plan; every reused file receives current metadata checks.
- The native UI clicked the actual Rescan button and captured both completed
  states. Its final status reported zero folders read and 2,003 files reused.
- 43 native PNGs generated: 18 laptop, 18 wide, five real live-scan stages and
  two cache stages. The real scanner's first preview arrived in 0.18 ms, with
  20 previews over five seconds. Native pause held the worker counter at
  235,258; resume and cancellation passed.
- 500,000-file stress recorded eight samples without invariant failure or
  layout-budget drift. Peak layout nodes: 194,917; peak RSS: 230.2 MB.
- Clippy completed with warnings, including existing UI/style warnings; this
  release does not claim a warning-free lint gate.

The agent opened and visually inspected these seven final-artifact captures:

- `laptop/hero-chrome-sunset.png`
- `laptop/treemap-light-golden-hour.png`
- `wide/extensions-tide-pool.png`
- `cache-ui/01-cold-scan.png`
- `cache-ui/02-verified-rescan.png`
- `live/03-paused.png`
- `live/04-resumed.png`

Button borders and labels remain readable in the reviewed dark/light states.
The cache completion summaries fit the footer. The paused view shows a static
indicator with Resume/Cancel; the resumed view restores the spinner and rate.

The final compact cache format stores names and one root path, rather than
repeating absolute paths per file. The fixture baseline occupied 124,458 bytes.
Large-drive cache storage remains bounded; scans exceeding the budget complete
normally without saving a baseline. Full-drive benchmarks were not run.

Tested Windows EXE: 7,822,848 bytes, file version 0.16.0.
SHA-256: `9921FD63EA6C89CCF701BEA3A1F1FF5A390D6F439BF09DF18EE6345A767BC663`.

Raw captures, inventories, timing data and receipts remain in the ignored
output directory above. Earlier intermediate builds were not published.

Website validation rendered the updated page in Chrome at 1440 x 1000 and
390 x 844. Both layouts loaded all nine images, reported no horizontal overflow
or script errors, and exposed v0.16.0 with matching direct EXE download links.
Desktop, mobile and Rescan-detail captures were visually reviewed. Website
captures and the browser receipt are in `test-results/website-v0.16.0/`.

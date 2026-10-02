# SpaceView v0.16.2 restore and scan validation

Command:

```powershell
./tools/gauntlet.ps1 -LivePath C:/ -SkipBuild -Output C:/trontstack/spaceview/test-results/20261002-v0.16.2-ship-final
```

Result: `[SpaceViewGauntlet] COMPLETE all automated checks passed`.

The restore gap occurred when a live snapshot replaced the layout during a
viewport change. Camera remapping previously depended on the discarded layout,
and stationary cameras were not clamped. Camera-owned bounds now remap current,
target and animation centers on every replacement. Invalid viewports are
ignored, and stationary cameras clamp before rendering. The title row has a
thin divider beneath it.

Cold scans overlap directory opens and initial metadata pages with four workers.
At most 32 cursors are prefetched; traversal order and complete final inventories
are preserved. Previews publish immediately, then about twice per second, with
clock checks amortized across entries. Final statistics hold references to the
largest 1,000 files instead of copying every filename/path and sorting millions
of records. Cached results retain the existing freshness checks.

The completed read-only C: probe traversed 5,185,812 files and 1,012,328 folders
in 267.854 seconds under concurrent system load. It received 513 bounded live
previews. This is a complete traversal measurement, without native GPU rendering
or final view-statistics computation. The drive inventory exceeded the cache
entry budget and was not saved. Runtime varies with load and filesystem warmth;
this measurement does not establish a controlled speedup over the old release.

After warming the same prefix, the gauntlet compared a 500,000-file target:

| Mode | Files | Seconds |
| --- | ---: | ---: |
| Cache identities and live previews | 501,393 | 2.359 |
| Cache identities, previews off | 500,369 | 2.269 |
| Ordinary traversal and live previews | 501,746 | 2.272 |

Live preview construction added about 4% elapsed time in this check. Small
overshoots occur while the probe polls progress; native rendering is measured
separately. Earlier sequential five-second probes reached different directory
phases and favored later warmed runs; the fixed-work guard keeps the same 70%
throughput threshold while comparing the same prefix.

- 57 Rust regressions passed, including stationary-camera restore, zoomed
  resize, and bounded statistics compared with a complete inventory and tied sizes.
- All 22 real NTFS cache mutation/fallback scenarios passed against fresh scans.
- Seven native C: captures covered growth, pause/resume, Windows minimize/restore,
  a smaller restored viewport, the original size, and cancellation. Eight preview
  updates and 209 rendered frames passed detail and viewport-coverage checks.
- An independent Windows observer confirmed the actual SpaceView window changed
  from restored to minimized and back. Final dark and light restored images were
  visually inspected; both filled the viewport and showed the title divider.
- 45 native PNGs were produced. Showcase and verified Rescan captures were also
  reviewed. Website desktop/mobile checks loaded all nine images with current
  version/download links and no horizontal overflow or script errors.
- The 500,000-file stress run recorded eight samples without invariant failures;
  peak materialized layout nodes were 112,287.

Packaged EXE: 7,883,264 bytes, file version 0.16.2.
SHA-256: `a44a0015ac04b5680b1a467210f7f7c5ab5889fc0197cf52360467b290d989f9`.

Raw receipts remain in the ignored output directories.

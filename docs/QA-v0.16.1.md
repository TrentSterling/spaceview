# SpaceView v0.16.1 scan-regression validation

```powershell
./tools/gauntlet.ps1 -LivePath C:/Github -SkipBuild -Output C:/trontstack/spaceview/test-results/20261002-v0.16.1-ship-final
```

Result: `[SpaceViewGauntlet] COMPLETE all automated checks passed`.

The shipped v0.16.0 cache-enabled native scan reproduced the report: 197 files
after 1,144.9 ms on `C:/Github`. The old live gauntlet disabled caching, so it
missed the expensive per-file read-data opens used to populate cache identities.

The final release uses bulk directory metadata with file IDs, omits DOS
short-name retrieval where supported, and retains compatible enumeration on
older drivers. Cached rescans still validate current metadata before publication.

Five-second probes on the same real tree recorded:

| Scanner | Files discovered |
| --- | ---: |
| Cache identities and live previews | 823,540 |
| Cache identities, no previews | 976,885 |
| Ordinary traversal and live previews | 1,040,309 |

The cache-enabled live probe averaged about 164,700 files/sec. These partial
scan measurements vary with directory shape, filesystem cache state and other
disk activity. They do not claim zero preview overhead or a full-drive timing.
The first preview arrived in 22.77 ms and 19 previews were received.

The native UI verified nine preview updates and 130 rendered frames without
collapsed visible detail. Pause held the worker at 319,343 files; resume and
cancellation passed. A separate regression keeps completed-folder detail
constant while the active branch moves between depths 1, 8 and 32. Snapshots
now use one size-weighted budget, rather than reducing completed detail with
the active stack's depth, and expand visible detail before drawing the frame.

- 54 Rust tests passed, including the new native multi-page/Unicode/file-ID,
  first-frame preview detail and active-depth regressions.
- All 22 native cache scenarios passed against fresh inventories, including
  hardlinks, open data writers, timestamp-only writers, real permission changes,
  root replacements, corrupt data, journal gaps/resets and cancellation.
- The 2,001-file cache fixture took 19.30 ms cold and 12.74 ms unchanged warm.
  The warm scan checked all 2,001 metadata records and enumerated zero folders.
  A fresh process reused the final 2,003-file fixture in 12.97 ms.
- 43 native screenshots were generated at two window sizes and in live/cache
  states. The agent visually reviewed final live growth/pause/resume captures,
  the dark showcase and the verified Rescan result. Completed folders retain
  visible detail when scanning moves into the Library branch.
- The 500,000-file stress run recorded eight samples without invariant failures;
  peak materialized layout nodes were 109,987.
- The updated website passed Chrome desktop/mobile checks: all nine images
  loaded, no horizontal overflow or script errors, and v0.16.1 download links.

Packaged EXE: 7,831,552 bytes, file version 0.16.1.
SHA-256: `e6cc2c7292cbb81b2ac840c106b478155b87fa7a278f8892e12049efa278938c`.

Raw receipts and screenshots remain in the ignored output directory above.

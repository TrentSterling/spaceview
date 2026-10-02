# Verified scan acceleration

SpaceView saves metadata after a completed NTFS scan and uses the Windows USN
change journal to plan later rescans. It does not immediately display an old
tree while hoping that it is still valid.

Open the same folder/drive again, or click **Rescan**. Right-click Rescan and
choose **Full scan** to traverse everything. Status text distinguishes checking
changes, refreshing, a verified refresh and a full scan.

## Validation and refresh

The baseline records the volume GUID/serial, root file reference, journal ID,
journal cursor and process security context (user, groups, privileges and
elevation). Journal coverage must include the entire interval since that cursor.
File references include their NTFS sequence number, so recycling an MFT slot
does not match the old entry. Version 2 and zero-extended NTFS version 3 journal
references are supported; unfamiliar formats force a full scan.

Changes mark the relevant parent folders for fresh enumeration and metadata
reads. Ancestors receive recalculated totals. Unchanged child folders are reused;
new, renamed or replaced folders are traversed. File-reference mappings include
every cached hardlink, including when a write arrives through an alias outside
the selected folder. Names from journal records are not needed.

The journal proves the folder traversal plan, but does not replace file
metadata reads. Windows can combine repeated writes into one reason while a
handle remains open; timestamp-only handles also bypass data-write sharing.
Before publishing a cached result, SpaceView reads current file identity, size
and last-write time along the cached paths. A bounded eight-thread pool uses
`NtQueryInformationByName` where available, with ordinary handle reads as a
fallback. Identity/type changes or failed queries require fresh discovery.
Incomplete branches and known data writers also receive fresh enumeration.
This preserves the acceleration structure while keeping metadata current.

Rescans therefore still perform work proportional to the number of cached
files. Their saving is in avoiding directory discovery and rebuilding unchanged
structure; they are not constant-time or zero-I/O scans. The "files reused"
counter describes reused structure, and the probe separately counts current
metadata reads. Speed depends on folder shape, disk and filesystem activity.

The baseline cursor precedes traversal. Refresh reads are also fenced before
they run, and journal changes during validation are replayed. No completion
cursor is assumed to cover reads made earlier. Continuously changing folders
eventually fall back to the ordinary live scan. Like a full scan, this is a
metadata view of a live filesystem, not an atomic filesystem snapshot.

## Fallback and storage

Journal access failure, unsupported filesystems, missing/truncated records,
journal reset, changed volume/root/security context, permission-change records,
unknown record layouts and corrupt caches all trigger full scans. Permission
changes affecting the selected tree, its hardlinks or its ancestors invalidate
reuse because inheritance can affect descendants without an event for each
child. Unknown ancestry uses the conservative volume-wide fallback. No elevation prompt or journal
creation/configuration is performed.

Caches live in `%LOCALAPPDATA%/SpaceView/scan-cache`, or under the isolated
`SPACEVIEW_PREFS_DIR` in the gauntlet. Each file has a format marker, length and
SHA-256 integrity checksum; decoded paths and totals are validated. Writes use
exclusive temporary files and atomic replacement. Storage is capped at eight
baselines / 512 MiB total, with a 256 MiB per-baseline limit. Failure to save a
cache does not fail the scan. The format stores the root path once and each
child name once, reconstructing and validating paths on load instead of
repeating absolute paths for millions of files. Validation caps decoded trees
at five million nodes and 512 levels. Cancellation does not replace the old baseline.

## Verification

`tools/gauntlet.ps1` runs the native cache fixture and compares inventories,
folder totals, file counts and timestamps with fresh scans after creates,
resizes, deletes, moves, hardlink changes, writes through external aliases and
repeated data and timestamp writes through open handles. It also tests real ACL changes,
replacement folders, altered security context, corrupt/checksum-invalid data,
journal discontinuities and cancellation while paused. Two native captures
exercise the actual Rescan button, and another process verifies disk reuse.

For a read-only timing probe of a real folder:

```powershell
spaceview.exe --cache-probe C:/some/folder --report-dir C:/some/report --force-full
spaceview.exe --cache-probe C:/some/folder --report-dir C:/some/report-warm
```

The probe reports elapsed time, folders/metadata read, files reused and totals.
It does not claim that every drive or change pattern has the same speedup.

Windows references: [journal identity](https://learn.microsoft.com/en-us/windows/win32/fileio/using-the-change-journal-identifier),
[journal bounds](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_journal_data_v0),
[record layout and reasons](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_record_v2),
[coalesced change records](https://learn.microsoft.com/en-us/windows/win32/fileio/change-journal-records),
[current file metadata](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_stat_information).

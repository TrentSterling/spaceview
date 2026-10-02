//! Read-only Windows journal access. Never create, resize or reset a journal.
use crate::scanner::ScanProgress;
use std::io;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Stamp {
    pub id: u64,
    pub serial: u32,
    pub size: u64,
    pub modified: u64,
    pub volatile: bool,
    pub is_dir: bool,
    pub reparse: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub volume: String,
    pub serial: u32,
    pub context: String,
    pub journal_id: u64,
    pub next: i64,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub id: u64,
    pub parent: u64,
    pub reason: u32,
}

pub fn compatible(old: &Checkpoint, now: &Checkpoint, first: i64, lowest: i64) -> bool {
    old.volume == now.volume
        && old.serial == now.serial
        && old.context == now.context
        && old.journal_id == now.journal_id
        && old.next >= first.max(lowest)
        && old.next >= 0
        && old.next <= now.next
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Parse every returned record, including records beyond the requested fence.
/// Unknown layouts or malformed buffers force a full scan, never skip a record.
fn parse_page(bytes: &[u8], start: i64) -> io::Result<(i64, Vec<Change>)> {
    if bytes.len() < 8 {
        return Err(invalid("short journal page"));
    }
    let next = i64::from_le_bytes(bytes[..8].try_into().unwrap());
    if next < start {
        return Err(invalid("journal cursor moved backwards"));
    }
    let mut offset = 8;
    let mut changes = Vec::new();
    while offset < bytes.len() {
        let record = &bytes[offset..];
        if record.len() < 60 {
            return Err(invalid("short journal record"));
        }
        let length = u32::from_le_bytes(record[..4].try_into().unwrap()) as usize;
        let major = u16::from_le_bytes(record[4..6].try_into().unwrap());
        let minor = u16::from_le_bytes(record[6..8].try_into().unwrap());
        let (minimum, parent, usn, reason) = match major {
            2 => (60, 16, 24, 40),
            3 => (76, 24, 40, 56),
            _ => return Err(invalid("unsupported journal version")),
        };
        if length < minimum || length > record.len() || length % 8 != 0 || minor != 0 {
            return Err(invalid(&format!(
                "unsupported journal record: version={major}.{minor} length={length} remaining={}",
                record.len()
            )));
        }
        // NTFS's 64-bit references are zero-extended in V3. Do not truncate a
        // genuine 128-bit reference and risk matching an unrelated cached file.
        if major == 3
            && (record[16..24].iter().any(|b| *b != 0) || record[32..40].iter().any(|b| *b != 0))
        {
            return Err(invalid("unsupported 128-bit file reference"));
        }
        let record_usn = i64::from_le_bytes(record[usn..usn + 8].try_into().unwrap());
        if record_usn < start || record_usn >= next {
            return Err(invalid("journal record outside cursor range"));
        }
        changes.push(Change {
            id: u64::from_le_bytes(record[8..16].try_into().unwrap()),
            parent: u64::from_le_bytes(record[parent..parent + 8].try_into().unwrap()),
            reason: u32::from_le_bytes(record[reason..reason + 4].try_into().unwrap()),
        });
        offset += length;
    }
    Ok((next, changes))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs::{File, OpenOptions};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::FILE_STAT_INFORMATION;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Ioctl::*;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub struct DirectoryEntry {
        pub name: std::ffi::OsString,
        pub stamp: Stamp,
    }

    /// File IDs, sizes and dates arrive in the same directory pages as names.
    /// Cold scans must not open/query every individual file to seed the cache.
    pub struct DirectoryEntries {
        file: File,
        buffer: Vec<u64>,
        offset: usize,
        next_page: bool,
        done: bool,
        extended: bool,
    }

    impl DirectoryEntries {
        fn page(&mut self, restart: bool) -> io::Result<()> {
            let class = match (self.extended, restart) {
                (true, true) => FileIdExtdDirectoryRestartInfo,
                (true, false) => FileIdExtdDirectoryInfo,
                (false, true) => FileIdBothDirectoryRestartInfo,
                (false, false) => FileIdBothDirectoryInfo,
            };
            if unsafe { GetFileInformationByHandleEx(self.file.as_raw_handle(), class,
                self.buffer.as_mut_ptr().cast(), (self.buffer.len() * 8) as u32) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    self.done = true;
                    return Ok(());
                }
                return Err(error);
            }
            self.offset = 0;
            self.next_page = false;
            Ok(())
        }
    }

    impl Iterator for DirectoryEntries {
        type Item = io::Result<DirectoryEntry>;
        fn next(&mut self) -> Option<Self::Item> {
            while !self.done {
                if self.next_page {
                    if let Err(error) = self.page(false) {
                        self.done = true;
                        return Some(Err(error));
                    }
                    if self.done { return None; }
                }
                let capacity = self.buffer.len() * 8;
                let (name_offset, record_size) = if self.extended {
                    (std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName), std::mem::size_of::<FILE_ID_EXTD_DIR_INFO>())
                } else {
                    (std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName), std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>())
                };
                if self.offset + record_size > capacity {
                    self.done = true;
                    return Some(Err(invalid("directory record outside buffer")));
                }
                let pointer = unsafe { self.buffer.as_ptr().cast::<u8>().add(self.offset) };
                let (length, next, id, size, modified, attributes) = if self.extended {
                    let info = unsafe { pointer.cast::<FILE_ID_EXTD_DIR_INFO>().read_unaligned() };
                    let id = if info.FileId.Identifier[8..].iter().all(|byte| *byte == 0) {
                        u64::from_le_bytes(info.FileId.Identifier[..8].try_into().unwrap())
                    } else { 0 }; // Never truncate an unsupported 128-bit identity.
                    (info.FileNameLength as usize, info.NextEntryOffset as usize, id,
                        info.EndOfFile, info.LastWriteTime, info.FileAttributes)
                } else {
                    let info = unsafe { pointer.cast::<FILE_ID_BOTH_DIR_INFO>().read_unaligned() };
                    (info.FileNameLength as usize, info.NextEntryOffset as usize, info.FileId as u64,
                        info.EndOfFile, info.LastWriteTime, info.FileAttributes)
                };
                if length == 0 || length % 2 != 0 || self.offset + name_offset + length > capacity
                    || (next != 0 && (next % 8 != 0 || next < name_offset + length || self.offset + next >= capacity))
                    || size < 0 {
                    self.done = true;
                    return Some(Err(invalid("invalid directory record")));
                }
                let name = std::ffi::OsString::from_wide(unsafe {
                    std::slice::from_raw_parts(pointer.add(name_offset).cast::<u16>(), length / 2)
                });
                self.next_page = next == 0;
                self.offset += next;
                if name == "." || name == ".." { continue; }
                return Some(Ok(DirectoryEntry { name, stamp: Stamp {
                    id, serial: 0, size: size as u64,
                    modified: (modified.max(0) as u64).saturating_sub(116_444_736_000_000_000) / 10_000_000,
                    volatile: false,
                    is_dir: attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                    reparse: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
                } }));
            }
            None
        }
    }

    pub fn directory_entries(path: &Path) -> io::Result<DirectoryEntries> {
        let file = OpenOptions::new().read(true)
            .access_mode(FILE_LIST_DIRECTORY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let mut entries = DirectoryEntries { file, buffer: vec![0; 8192],
            offset: 0, next_page: true, done: false, extended: true };
        // The extended class omits DOS short-name retrieval. Older drivers
        // retain the compatible listing rather than reverting to file opens.
        if entries.page(true).is_err() {
            entries.extended = false;
            entries.page(true)?;
        }
        Ok(entries)
    }

    // Query current metadata along cached paths without opening a file handle.
    // Older Windows/filesystems use the ordinary handle-based stamp instead.
    pub fn fresh_stamp(path: &Path) -> io::Result<Stamp> {
        type Query = unsafe extern "system" fn(
            *const OBJECT_ATTRIBUTES,
            *mut IO_STATUS_BLOCK,
            *mut std::ffi::c_void,
            u32,
            i32,
        ) -> i32;
        static QUERY: std::sync::OnceLock<Option<Query>> = std::sync::OnceLock::new();
        let query = QUERY.get_or_init(|| unsafe {
            let module = GetModuleHandleW(windows_sys::core::w!("ntdll.dll"));
            GetProcAddress(module, windows_sys::core::s!("NtQueryInformationByName")).map(
                |function| {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(function)
                },
            )
        });
        let Some(query) = query else {
            return stamp(path);
        };
        let original: Vec<u16> = path.as_os_str().encode_wide().collect();
        let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
        let suffix = if original.starts_with(&prefix) {
            &original[4..]
        } else if original.get(1) == Some(&(b':' as u16)) {
            original.as_slice()
        } else {
            return stamp(path);
        };
        if suffix.is_empty() {
            return stamp(path);
        }
        let mut name: Vec<u16> = r"\??\"
            .encode_utf16()
            .chain(suffix.iter().copied())
            .collect();
        if name.len() > 32766 {
            return stamp(path);
        }
        let string = UNICODE_STRING {
            Length: (name.len() * 2) as u16,
            MaximumLength: (name.len() * 2) as u16,
            Buffer: name.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: std::ptr::null_mut(),
            ObjectName: &string,
            Attributes: 0x40 | 0x1000,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        let mut info: FILE_STAT_INFORMATION = unsafe { std::mem::zeroed() };
        let result = unsafe {
            query(
                &attributes,
                &mut status,
                std::ptr::addr_of_mut!(info).cast(),
                std::mem::size_of_val(&info) as u32,
                68,
            )
        };
        if result != 0 || info.EndOfFile < 0 {
            return stamp(path);
        }
        Ok(Stamp {
            id: info.FileId as u64,
            serial: 0,
            size: info.EndOfFile as u64,
            modified: (info.LastWriteTime.max(0) as u64).saturating_sub(116_444_736_000_000_000)
                / 10_000_000,
            volatile: false,
            is_dir: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            reparse: info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        })
    }

    pub fn stamp(path: &Path) -> io::Result<Stamp> {
        let open = |access, share| {
            OpenOptions::new()
                .read(true)
                .access_mode(access)
                .share_mode(share)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
        };
        // Rescans always verify current metadata, including open writers.
        // Read-data access is unnecessary and triggers expensive content-open
        // work in filesystem filters during a cold scan.
        let file = open(
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let modified = ((info.ftLastWriteTime.dwHighDateTime as u64) << 32)
            | info.ftLastWriteTime.dwLowDateTime as u64;
        Ok(Stamp {
            id: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
            serial: info.dwVolumeSerialNumber,
            size: ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64,
            modified: modified.saturating_sub(116_444_736_000_000_000) / 10_000_000,
            volatile: false,
            is_dir: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            reparse: info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        })
    }

    fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
        let mut length = 0;
        unsafe {
            GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 || length > 1_048_576 {
            return Err(invalid("invalid security context"));
        }
        let mut data = vec![0u64; (length as usize + 7) / 8];
        if unsafe {
            GetTokenInformation(token, class, data.as_mut_ptr().cast(), length, &mut length)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(data)
    }

    fn context() -> io::Result<String> {
        let mut handle = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { File::from_raw_handle(handle) };
        let mut hash = Sha256::new();
        let user = token_info(handle, TokenUser)?;
        let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        hash.update(unsafe {
            std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize)
        });
        let groups = token_info(handle, TokenGroups)?;
        let group = groups.as_ptr().cast::<TOKEN_GROUPS>();
        let count = unsafe { (*group).GroupCount } as usize;
        if count > 16384 {
            return Err(invalid("invalid security groups"));
        }
        let entries = unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!((*group).Groups).cast::<SID_AND_ATTRIBUTES>(),
                count,
            )
        };
        let mut values: Vec<Vec<u8>> = entries
            .iter()
            .map(|entry| {
                let mut value = entry.Attributes.to_le_bytes().to_vec();
                value.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(
                        entry.Sid.cast::<u8>(),
                        GetLengthSid(entry.Sid) as usize,
                    )
                });
                value
            })
            .collect();
        values.sort();
        for value in values {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value);
        }
        for class in [TokenElevation, TokenPrivileges] {
            let info = token_info(handle, class)?;
            hash.update(unsafe {
                std::slice::from_raw_parts(info.as_ptr().cast::<u8>(), info.len() * 8)
            });
        }
        drop(token);
        Ok(format!("{:x}", hash.finalize()))
    }

    pub struct Journal {
        file: File,
        volume: String,
        serial: u32,
        context: String,
    }
    impl Journal {
        pub fn open(path: &Path) -> io::Result<Self> {
            let path = wide(path);
            let mut mount = vec![0u16; 1024];
            if unsafe { GetVolumePathNameW(path.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut volume = vec![0u16; 128];
            let mut fs = vec![0u16; 32];
            let mut serial = 0;
            if unsafe {
                GetVolumeNameForVolumeMountPointW(
                    mount.as_ptr(),
                    volume.as_mut_ptr(),
                    volume.len() as u32,
                )
            } == 0
                || unsafe {
                    GetVolumeInformationW(
                        mount.as_ptr(),
                        std::ptr::null_mut(),
                        0,
                        &mut serial,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        fs.as_mut_ptr(),
                        fs.len() as u32,
                    )
                } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let string = |chars: &[u16]| {
                String::from_utf16_lossy(
                    &chars[..chars.iter().position(|c| *c == 0).unwrap_or(chars.len())],
                )
            };
            if string(&fs) != "NTFS" {
                return Err(invalid("change journal requires NTFS"));
            }
            // Query/read through the filesystem root, not a raw storage handle.
            let file = OpenOptions::new()
                .read(true)
                .access_mode(0)
                .share_mode(7)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(string(&mount))?;
            let journal = Self {
                file,
                volume: string(&volume),
                serial,
                context: context()?,
            };
            journal.query()?;
            Ok(journal)
        }

        fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> io::Result<usize> {
            let mut count = 0;
            if unsafe {
                DeviceIoControl(
                    self.file.as_raw_handle(),
                    code,
                    if input.is_empty() {
                        std::ptr::null()
                    } else {
                        input.as_ptr().cast()
                    },
                    input.len() as u32,
                    output.as_mut_ptr().cast(),
                    output.len() as u32,
                    &mut count,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(count as usize)
        }

        pub fn query(&self) -> io::Result<(Checkpoint, i64, i64)> {
            let mut output = [0u8; 80];
            let length = self.ioctl(FSCTL_QUERY_USN_JOURNAL, &[], &mut output)?;
            if length < 56 {
                return Err(invalid("short journal information"));
            }
            let u = |offset| u64::from_le_bytes(output[offset..offset + 8].try_into().unwrap());
            Ok((
                Checkpoint {
                    volume: self.volume.clone(),
                    serial: self.serial,
                    context: self.context.clone(),
                    journal_id: u(0),
                    next: u(16) as i64,
                },
                u(8) as i64,
                u(24) as i64,
            ))
        }

        pub fn changes(
            &self,
            old: &Checkpoint,
            progress: &ScanProgress,
        ) -> io::Result<(Checkpoint, Vec<Change>)> {
            let (target, first, lowest) = self.query()?;
            if !compatible(old, &target, first, lowest) {
                return Err(invalid("journal continuity lost"));
            }
            let mut cursor = old.next;
            let mut changes = Vec::new();
            let mut output = vec![0u8; 65536];
            while cursor < target.next {
                if !progress.keep_scanning() {
                    return Err(io::Error::from(io::ErrorKind::Interrupted));
                }
                let mut input = [0u8; 48];
                input[..8].copy_from_slice(&cursor.to_le_bytes());
                input[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
                input[32..40].copy_from_slice(&old.journal_id.to_le_bytes());
                input[40..42].copy_from_slice(&2u16.to_le_bytes());
                input[42..44].copy_from_slice(&4u16.to_le_bytes());
                // Both APIs return the complete journal stream. Unprivileged
                // reads redact names; planning uses file/parent IDs exclusively.
                let length =
                    self.ioctl(FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, &input, &mut output)?;
                let (next, page) = parse_page(&output[..length], cursor)?;
                if next <= cursor {
                    return Err(invalid("journal did not advance"));
                }
                changes.extend(page);
                if changes.len() > 250_000 {
                    return Err(invalid("change history exceeds refresh budget"));
                }
                cursor = next;
            }
            let (end, first, lowest) = self.query()?;
            if !compatible(old, &end, first, lowest) {
                return Err(invalid("journal changed while reading"));
            }
            Ok((target, changes))
        }
    }
}

#[cfg(windows)]
pub use windows::{directory_entries, fresh_stamp, stamp, Journal};

#[cfg(not(windows))]
pub fn stamp(_: &Path) -> io::Result<Stamp> {
    Err(invalid("Windows metadata unavailable"))
}
#[cfg(not(windows))]
pub fn fresh_stamp(path: &Path) -> io::Result<Stamp> {
    stamp(path)
}
#[cfg(not(windows))]
pub struct Journal;
#[cfg(not(windows))]
impl Journal {
    pub fn open(_: &Path) -> io::Result<Self> {
        Err(invalid("Windows journal unavailable"))
    }
    pub fn query(&self) -> io::Result<(Checkpoint, i64, i64)> {
        Err(invalid("Windows journal unavailable"))
    }
    pub fn changes(
        &self,
        _: &Checkpoint,
        _: &ScanProgress,
    ) -> io::Result<(Checkpoint, Vec<Change>)> {
        Err(invalid("Windows journal unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_gaps_resets_volume_and_security_changes() {
        let old = Checkpoint {
            volume: "drive".into(),
            serial: 7,
            context: "user".into(),
            journal_id: 1,
            next: 100,
        };
        let mut now = old.clone();
        now.next = 200;
        assert!(compatible(&old, &now, 50, 0));
        assert!(!compatible(&old, &now, 101, 0));
        assert!(!compatible(&old, &now, 50, 101));
        now.journal_id = 2;
        assert!(!compatible(&old, &now, 50, 0));
        now = old.clone();
        now.volume = "replacement".into();
        assert!(!compatible(&old, &now, 0, 0));
        now = old.clone();
        now.context = "elevated".into();
        assert!(!compatible(&old, &now, 0, 0));
        now = old.clone();
        now.next = 99;
        assert!(!compatible(&old, &now, 0, 0));
    }
    #[test]
    fn parser_rejects_unknown_short_and_backwards_records() {
        let mut page = 200i64.to_le_bytes().to_vec();
        let mut record = [0u8; 64];
        record[..4].copy_from_slice(&64u32.to_le_bytes());
        record[4] = 2;
        record[8..16].copy_from_slice(&9u64.to_le_bytes());
        record[16..24].copy_from_slice(&8u64.to_le_bytes());
        record[24..32].copy_from_slice(&100i64.to_le_bytes());
        record[40] = 1;
        page.extend(record);
        let (next, changes) = parse_page(&page, 100).unwrap();
        assert_eq!(next, 200);
        assert_eq!(changes[0].id, 9);
        assert_eq!(changes[0].parent, 8);
        page[12] = 4;
        assert!(parse_page(&page, 100).is_err());
        assert!(parse_page(&page[..20], 100).is_err());
        assert!(parse_page(&99i64.to_le_bytes(), 100).is_err());
    }
    #[test]
    fn v3_ids_are_checked_before_conversion() {
        let mut page = 200i64.to_le_bytes().to_vec();
        let mut record = [0u8; 80];
        record[..4].copy_from_slice(&80u32.to_le_bytes());
        record[4] = 3;
        record[8..16].copy_from_slice(&11u64.to_le_bytes());
        record[24..32].copy_from_slice(&9u64.to_le_bytes());
        record[40..48].copy_from_slice(&100i64.to_le_bytes());
        record[56] = 4;
        page.extend(record);
        let (_, changes) = parse_page(&page, 100).unwrap();
        assert_eq!(changes[0].id, 11);
        assert_eq!(changes[0].parent, 9);
        assert_eq!(changes[0].reason, 4);
        page[24] = 1;
        assert!(parse_page(&page, 100).is_err());
    }
}

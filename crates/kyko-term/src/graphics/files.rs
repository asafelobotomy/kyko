//! Reading the images a program names by file (`t=f`, `t=t`), with the checks that keep it to
//! regular and temporary files.

use super::*;

/// A file the program named, for `t=f` and `t=t`: a regular file only, never a device or a
/// `/proc` or `/sys` entry; a temporary file must sit directly in a temporary folder and have
/// `tty-graphics-protocol` in its own name, and it is removed after reading. The checks are on
/// the real path (symlinks and `..` resolved), so `/tmp/../home/...` or a link in `/tmp` can
/// neither read a device nor have a file outside the temporary folders removed.
pub(crate) fn read_named(path: &str, temporary: bool, c: &Command) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let real = std::fs::canonicalize(path).map_err(|e| format!("EBADF:{e}"))?;
    let p = real.as_path();
    let meta = std::fs::metadata(p).map_err(|e| format!("EBADF:{e}"))?;
    if !meta.is_file() {
        return Err("EINVAL:not a regular file".into());
    }
    if temporary {
        let named = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains("tty-graphics-protocol"));
        let in_tmp = p
            .parent()
            .is_some_and(|d| temp_dirs().iter().any(|t| t == d));
        if !named || !in_tmp {
            return Err("EPERM:not a temporary file for the graphics protocol".into());
        }
    } else if ["/proc", "/sys", "/dev"].iter().any(|d| p.starts_with(d)) {
        return Err("EINVAL:not a regular file".into());
    }
    let mut f = std::fs::File::open(p).map_err(|e| format!("EBADF:{e}"))?;
    if c.offset_bytes > 0 {
        f.seek(SeekFrom::Start(u64::from(c.offset_bytes)))
            .map_err(|e| format!("EBADF:{e}"))?;
    }
    let limit = if c.size > 0 {
        u64::from(c.size)
    } else {
        QUOTA as u64
    };
    let mut out = Vec::new();
    f.take(limit)
        .read_to_end(&mut out)
        .map_err(|e| format!("EBADF:{e}"))?;
    if temporary {
        let _ = std::fs::remove_file(p);
    }
    Ok(out)
}

/// The folders a temporary file may be in, as real paths: `/tmp`, `/dev/shm` and `$TMPDIR`.
pub(crate) fn temp_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = ["/tmp", "/dev/shm"].map(Into::into).to_vec();
    if let Some(t) = std::env::var_os("TMPDIR").filter(|t| !t.is_empty()) {
        dirs.push(t.into());
    }
    dirs.into_iter()
        .filter_map(|d| std::fs::canonicalize(d).ok())
        .collect()
}

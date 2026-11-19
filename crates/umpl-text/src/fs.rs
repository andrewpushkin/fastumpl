use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Writes `contents` to `path` without ever leaving a half-written file:
/// the data goes to a temporary file in the same folder, which is then
/// renamed over the target. A symbolic link is followed, so the file it
/// points to is written and the link stays; the file keeps its permissions.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let resolved;
    let path = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            // A link to a file not there yet: write where it points.
            resolved = fs::canonicalize(path).or_else(|_| {
                fs::read_link(path).map(|t| path.parent().unwrap_or(Path::new(".")).join(t))
            })?;
            resolved.as_path()
        }
        _ => path,
    };
    let permissions = fs::metadata(path).ok().map(|m| m.permissions());
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        drop(f);
        if let Some(p) = &permissions {
            fs::set_permissions(&tmp, p.clone())?;
        }
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let dir = std::env::temp_dir().join(format!("umpl-fs-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.umpl");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "two");
        let leftovers = fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}

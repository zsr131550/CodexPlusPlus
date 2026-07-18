use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::Context;
use fs2::FileExt;

pub struct CoordinationLock {
    file: File,
}

impl Drop for CoordinationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub fn acquire_exclusive(path: &Path) -> anyhow::Result<CoordinationLock> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create lock directory {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open coordination lock {}", path.display()))?;
    FileExt::lock_exclusive(&file)
        .with_context(|| format!("failed to lock coordination path {}", path.display()))?;
    Ok(CoordinationLock { file })
}

pub fn sidecar_path(path: &Path) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".lock");
    PathBuf::from(sidecar)
}

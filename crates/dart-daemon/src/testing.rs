//! Test utilities and fixtures for the daemon.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// An isolated temporary directory created for tests and cleaned up on drop.
pub struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    /// Creates a new uniquely named temporary directory with the given label prefix.
    pub fn new(label: &str) -> Self {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "dart-{label}-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test directory");
        Self { path }
    }

    /// Returns the path to the temporary directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

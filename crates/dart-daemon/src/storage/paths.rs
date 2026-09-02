//! The on-disk locations owned and managed by Dart.
//!
//! This module is the source of truth for Dart's filesystem layout.
//! Callers pass around [`DartPaths`] instead of assembling `$DART_HOME`
//! path strings in multiple locations.

use std::path::{Path, PathBuf};

/// Filesystem layout for Dart instances and runtime caches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DartPaths {
    home: PathBuf,
}

impl DartPaths {
    /// Creates a new paths layout rooted at `home`.
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    /// Returns the root Dart data directory (`$DART_HOME`).
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Returns the directory where server instances reside (`$DART_HOME/instances`).
    pub fn instances_dir(&self) -> PathBuf {
        self.home.join("instances")
    }

    /// Returns the directory where cached Fabric runtimes reside (`$DART_HOME/runtimes/fabric`).
    pub fn fabric_runtimes_dir(&self) -> PathBuf {
        self.home.join("runtimes").join("fabric")
    }
}

#[cfg(test)]
mod tests {
    use super::DartPaths;
    use std::path::PathBuf;

    #[test]
    fn defines_the_documented_data_layout() {
        let paths = DartPaths::new(PathBuf::from("dart-data"));

        assert_eq!(paths.instances_dir(), PathBuf::from("dart-data/instances"));
        assert_eq!(
            paths.fabric_runtimes_dir(),
            PathBuf::from("dart-data/runtimes/fabric")
        );
    }
}

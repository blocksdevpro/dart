//! The on-disk locations that Dart owns.
//!
//! Keep path construction here. Callers receive a `DartPaths` value instead of
//! reassembling `$DART_HOME` layout strings in several modules.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct DartPaths {
    home: PathBuf,
}

impl DartPaths {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.home.join("instances")
    }

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

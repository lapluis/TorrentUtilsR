use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "torrent_utils_test_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    // Only the CLI compatibility tests need this path.
    #[allow(dead_code)]
    pub(crate) fn missing_config(&self) -> String {
        self.0.join("missing-config.toml").display().to_string()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};

pub fn workspace_tempdir() -> TempDir {
    Builder::new()
        .prefix(".appleby-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .expect("create temporary directory in workspace")
}

pub fn fixture_path(path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path)
}

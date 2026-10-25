use std::path::{Path, PathBuf};

use crate::common;
use common::fake_env::FakeEnv;

#[test]
fn project_config_search_dedups_multiple_files_without_parent() {
    let envx = FakeEnv::new().with_file(".yamllint", "rules: {}\n");
    let inputs = vec![PathBuf::from("a.yml"), PathBuf::from("b.yml")];
    let ctx = ryl::config::discover_config_with(
        &inputs,
        &ryl::config::Overrides::default(),
        &envx,
    )
    .expect("ok");
    assert_eq!(ctx.source.as_deref(), Some(Path::new(".yamllint")));
}

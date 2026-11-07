use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::{
    SystemEnv, TOML_PROJECT_CONFIG_CANDIDATES, YAML_PROJECT_CONFIG_CANDIDATES,
    candidate_path, find_project_config_core,
};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Preset {
    Default,
    Relaxed,
}

impl Preset {
    fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Relaxed => "relaxed",
        }
    }

    fn toml(self) -> &'static str {
        match self {
            Self::Default => include_str!("init/presets/default.toml"),
            Self::Relaxed => include_str!("init/presets/relaxed.toml"),
        }
    }
}

/// Create `.ryl.toml` exclusively, returning the project config it supersedes.
///
/// # Errors
/// Returns an error if a local config exists, discovery fails, or I/O fails.
pub fn write_config(dir: &Path, preset: Preset) -> Result<Option<PathBuf>, String> {
    for name in TOML_PROJECT_CONFIG_CANDIDATES
        .into_iter()
        .chain(YAML_PROJECT_CONFIG_CANDIDATES)
    {
        let path = candidate_path(dir, name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "error: cannot inspect {}: {error}",
                    path.display()
                ));
            }
            Ok(_) => {}
        }
        if name == "pyproject.toml" {
            let table = fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    text.parse::<toml::Table>()
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| {
                    format!("error: cannot read {}: {error}", path.display())
                })?;
            if table.get("tool").and_then(|tool| tool.get("ryl")).is_none() {
                continue;
            }
        }
        let migration = if YAML_PROJECT_CONFIG_CANDIDATES.contains(&name) {
            "; use 'ryl --migrate-configs' to preview migration and '--migrate-write' to apply it"
        } else {
            ""
        };
        return Err(format!(
            "error: configuration already exists: {}{migration}",
            path.display()
        ));
    }
    let ancestor = find_project_config_core(&SystemEnv, &[dir.to_path_buf()])?
        .map(|found| found.cfg_path);
    let path = dir.join(".ryl.toml");
    let content = format!(
        "# {} preset, written by ryl {}\n\n{}",
        preset.name(),
        env!("CARGO_PKG_VERSION"),
        preset.toml()
    );
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .map_err(|error| format!("error: cannot write {}: {error}", path.display()))?;
    Ok(ancestor)
}

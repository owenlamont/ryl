use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::config::{Overrides, discover_config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteMode {
    Preview,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputMode {
    SummaryOnly,
    IncludeToml,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCleanup {
    Keep,
    Delete,
    RenameSuffix(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateOptions {
    pub root: PathBuf,
    pub write_mode: WriteMode,
    pub output_mode: OutputMode,
    pub cleanup: SourceCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEntry {
    pub source: PathBuf,
    pub target: PathBuf,
    pub toml: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateResult {
    pub entries: Vec<MigrationEntry>,
    pub cleanup_only_sources: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct MigrationPlan {
    entries: Vec<MigrationEntry>,
    cleanup_only_sources: Vec<PathBuf>,
    warnings: Vec<String>,
}

/// Apply write + cleanup actions for already planned migration entries.
///
/// # Errors
/// Returns an error if writing targets or requested source cleanup fails.
pub fn apply_migration_entries(
    entries: &[MigrationEntry],
    cleanup_only_sources: &[PathBuf],
    cleanup: &SourceCleanup,
) -> Result<(), String> {
    let apply_cleanup = |source: &Path| -> Result<(), String> {
        match cleanup {
            SourceCleanup::Keep => {}
            SourceCleanup::Delete => {
                fs::remove_file(source).map_err(|err| {
                    format!(
                        "failed to delete migrated source config {}: {err}",
                        source.display()
                    )
                })?;
            }
            SourceCleanup::RenameSuffix(suffix) => {
                let source_name = source
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().to_string());
                let renamed = source.with_file_name(format!("{source_name}{suffix}"));
                fs::rename(source, &renamed).map_err(|err| {
                    format!(
                        "failed to rename migrated source config {} to {}: {err}",
                        source.display(),
                        renamed.display()
                    )
                })?;
            }
        }
        Ok(())
    };

    for entry in entries {
        fs::write(&entry.target, &entry.toml).map_err(|err| {
            format!(
                "failed to write migrated config {}: {err}",
                entry.target.display()
            )
        })?;
    }

    for source in entries
        .iter()
        .map(|entry| &entry.source)
        .chain(cleanup_only_sources.iter())
    {
        apply_cleanup(source)?;
    }

    Ok(())
}

fn yaml_config_rank(path: &Path) -> usize {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".yamllint") => 0,
        Some(".yamllint.yaml") => 1,
        _ => 2,
    }
}

fn is_legacy_yaml_config_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == ".yamllint" || name == ".yamllint.yaml" || name == ".yamllint.yml"
        })
}

fn discover_legacy_yaml_configs(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return if is_legacy_yaml_config_path(root) {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        };
    }

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .build();

    walker
        .flatten()
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| path.is_file() && is_legacy_yaml_config_path(path))
        .collect()
}

fn build_migration_entries(root: &Path) -> Result<MigrationPlan, String> {
    let mut grouped: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for path in discover_legacy_yaml_configs(root) {
        let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        grouped.entry(parent).or_default().push(path);
    }

    let mut plan = MigrationPlan::default();
    let mut directories: Vec<PathBuf> = grouped.keys().cloned().collect();
    directories.sort();

    for dir in directories {
        let mut paths = grouped
            .remove(&dir)
            .expect("directory key should exist in grouped map");
        paths.sort_by(|left, right| {
            yaml_config_rank(left)
                .cmp(&yaml_config_rank(right))
                .then(left.cmp(right))
        });
        let primary = paths
            .first()
            .cloned()
            .expect("at least one config path should exist per grouped directory");
        for ignored in paths.iter().skip(1) {
            plan.cleanup_only_sources.push(ignored.clone());
            plan.warnings.push(format!(
                "warning: skipping lower-precedence config {} in favor of {}",
                ignored.display(),
                primary.display()
            ));
        }

        let ctx = discover_config(
            &[],
            &Overrides {
                config_file: Some(primary.clone()),
                config_data: None,
            },
        )?;
        let rendered = ctx.config.to_toml_string()?;
        let toml = format!("{}\n", rendered.trim_end());
        plan.entries.push(MigrationEntry {
            source: primary,
            target: dir.join(".ryl.toml"),
            toml,
        });
    }

    Ok(plan)
}

/// Build and optionally apply YAML-to-TOML config migration.
///
/// # Errors
/// Returns an error if migration planning fails or file operations fail in write mode.
pub fn migrate_configs(options: &MigrateOptions) -> Result<MigrateResult, String> {
    if !options.root.exists() {
        return Err(format!(
            "error: migrate root does not exist: {}",
            options.root.display()
        ));
    }

    let plan = build_migration_entries(&options.root)?;
    if options.write_mode == WriteMode::Write {
        apply_migration_entries(&plan.entries, &plan.cleanup_only_sources, &options.cleanup)?;
    }

    Ok(MigrateResult {
        entries: plan.entries,
        cleanup_only_sources: plan.cleanup_only_sources,
        warnings: plan.warnings,
    })
}

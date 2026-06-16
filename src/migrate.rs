use std::collections::HashMap;
use std::fs;
use std::io::Write;
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

/// A yamllint user-global config to migrate to ryl's own user-global location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserConfigMigration {
    pub source: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateOptions {
    /// Project tree to scan for legacy YAML configs; `None` skips project migration.
    pub project_root: Option<PathBuf>,
    /// User-global config to migrate; `None` skips it.
    pub user_config: Option<UserConfigMigration>,
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
/// Returns an error if creating the target directory, writing targets, or requested
/// source cleanup fails.
pub fn apply_migration_entries(
    entries: &[MigrationEntry],
    cleanup_only_sources: &[PathBuf],
    cleanup: &SourceCleanup,
) -> Result<(), String> {
    // Preflight rename-backup collisions before writing any target, so a collision never
    // leaves migrated targets behind (a retry would then skip them as already migrated).
    // Symlinked sources are excluded because cleanup never renames them.
    if let SourceCleanup::RenameSuffix(suffix) = cleanup {
        for source in entries
            .iter()
            .map(|entry| entry.source.as_path())
            .chain(cleanup_only_sources.iter().map(PathBuf::as_path))
        {
            if is_symlink(source) {
                continue;
            }
            let renamed = rename_destination(source, suffix);
            if fs::symlink_metadata(&renamed).is_ok() {
                return Err(format!(
                    "refusing to overwrite existing backup {} when renaming {}",
                    renamed.display(),
                    source.display()
                ));
            }
        }
    }

    let apply_cleanup = |source: &Path| -> Result<(), String> {
        // Never delete or rename through a symlink: acting on the link's target would
        // orphan the real file, so a symlinked source is preserved untouched (mirrors
        // the --fix/--diff symlink refusal).
        if is_symlink(source) {
            return Ok(());
        }
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
                // The backup-collision refusal is preflighted before any target is written.
                let renamed = rename_destination(source, suffix);
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
        // The user-global target dir (e.g. ~/.config/ryl/) may not exist yet; project
        // targets land beside an existing config so this is a no-op there. A target with
        // no parent component (never produced by the planner) simply skips dir creation
        // and lets the write below report any failure — so this can never panic.
        if let Some(parent) = entry.target.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create directory {} for migrated config: {err}",
                    parent.display()
                )
            })?;
        }
        // `create_new` refuses (atomically, without following a symlink) to overwrite a
        // target that already exists, a backstop for a target that appears between planning
        // and writing so a stale plan can never clobber an existing config or symlink.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&entry.target)
            .and_then(|mut file| file.write_all(entry.toml.as_bytes()))
            .map_err(|err| {
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

/// Whether `path`'s final component is a symlink (does not resolve parents), matching the
/// `--fix`/`--diff` symlink check.
fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// The backup path a `RenameSuffix` cleanup would move `source` to (its name + suffix).
fn rename_destination(source: &Path, suffix: &str) -> PathBuf {
    let name = source
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().to_string());
    source.with_file_name(format!("{name}{suffix}"))
}

/// An existing ryl-native TOML config *file* in `target`'s directory that a migration into
/// `target` would overwrite or be shadowed by. Covers the root names (`.ryl.toml` outranks
/// `ryl.toml` in discovery, so either is a collision) and the repo-local `.config/`
/// candidates (`.config/.ryl.toml`/`.config/ryl.toml`): migrating writes `<dir>/.ryl.toml`,
/// which outranks an existing `.config/` config and would silently shadow it, so that is a
/// collision too. A non-file (e.g. a directory) is not treated as a collision so the write
/// path still reports it.
fn existing_ryl_native_config(target: &Path) -> Option<PathBuf> {
    target
        .parent()
        .into_iter()
        .flat_map(|dir| {
            [
                dir.join(".ryl.toml"),
                dir.join("ryl.toml"),
                dir.join(".config").join(".ryl.toml"),
                dir.join(".config").join("ryl.toml"),
            ]
        })
        .find(|candidate| candidate.is_file())
}

/// Convert one YAML config at `source` into a TOML `MigrationEntry` written to `target`,
/// flagging a migrated config that ends up enabling no rules. Shared by project and
/// user-global migration. Skips (with a warning) rather than migrating when the source or
/// target is a symlink, or when a ryl-native config already exists at the target, so a
/// migration never follows a symlink, clobbers an existing config, or is silently shadowed.
/// Returns `true` when an entry was added, `false` when the migration was skipped — callers
/// use this to avoid cleaning up sources for a directory that was not migrated.
///
/// `user_global` marks the user-global config, whose target moves to a different directory
/// (`<config-dir>/ryl/`): a top-level `ignore-from-file` is inlined so the relative path
/// does not dangle, and a rule-level `ignore-from-file` (which cannot be relocated without
/// rewriting the rule config) is refused.
fn build_entry(
    source: &Path,
    target: PathBuf,
    plan: &mut MigrationPlan,
    user_global: bool,
) -> Result<bool, String> {
    if is_symlink(source) {
        plan.warnings.push(format!(
            "warning: skipping {}: refusing to follow a symlink",
            source.display()
        ));
        return Ok(false);
    }
    if is_symlink(&target) {
        plan.warnings.push(format!(
            "warning: skipping migration to {}: refusing to follow a symlink",
            target.display()
        ));
        return Ok(false);
    }
    if let Some(existing) = existing_ryl_native_config(&target) {
        plan.warnings.push(format!(
            "warning: skipping migration of {}: a ryl-native config already exists at {}",
            source.display(),
            existing.display()
        ));
        return Ok(false);
    }
    let mut ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(source.to_path_buf()),
            config_data: None,
        },
    )?;
    if user_global {
        if ctx.config.has_relative_rule_level_ignore_from_file() {
            plan.warnings.push(format!(
                "warning: skipping migration of {}: a relative rule-level ignore-from-file \
                 cannot be relocated to the ryl user-global config; inline the patterns or \
                 use an absolute path, then re-run",
                source.display()
            ));
            return Ok(false);
        }
        // The target moves to <config-dir>/ryl/, so a relative top-level ignore-from-file
        // would dangle; inline its already-resolved patterns to keep the config working.
        ctx.config.inline_resolved_ignore_from_file();
    }
    if !ctx.config.enables_any_rule() {
        plan.warnings.push(format!(
            "warning: migrated config {} enables no rules; ryl will not lint with it \
             \u{2014} enable at least one rule, or use 'extends: default' for the standard rule set",
            target.display()
        ));
    }
    let rendered = ctx.config.to_toml_string();
    let toml = format!("{}\n", rendered.trim_end());
    plan.entries.push(MigrationEntry {
        source: source.to_path_buf(),
        target,
        toml,
    });
    Ok(true)
}

fn build_project_entries(root: &Path, plan: &mut MigrationPlan) -> Result<(), String> {
    let mut grouped: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for path in discover_legacy_yaml_configs(root) {
        let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        grouped.entry(parent).or_default().push(path);
    }

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
        // Only enqueue lower-precedence siblings for cleanup once the primary actually
        // migrated; otherwise a skipped directory (collision/symlink) would still delete
        // or rename its siblings with --delete-old/--rename-old.
        if build_entry(&primary, dir.join(".ryl.toml"), plan, false)? {
            for ignored in paths.iter().skip(1) {
                plan.cleanup_only_sources.push(ignored.clone());
                plan.warnings.push(format!(
                    "warning: skipping lower-precedence config {} in favor of {}",
                    ignored.display(),
                    primary.display()
                ));
            }
        }
    }

    Ok(())
}

/// Build and optionally apply YAML-to-TOML config migration.
///
/// # Errors
/// Returns an error if migration planning fails or file operations fail in write mode.
pub fn migrate_configs(options: &MigrateOptions) -> Result<MigrateResult, String> {
    let mut plan = MigrationPlan::default();
    if let Some(root) = &options.project_root {
        if !root.exists() {
            return Err(format!(
                "error: migrate root does not exist: {}",
                root.display()
            ));
        }
        build_project_entries(root, &mut plan)?;
    }
    if let Some(user) = &options.user_config
        && user.source.exists()
    {
        build_entry(&user.source, user.target.clone(), &mut plan, true)?;
    }
    if options.write_mode == WriteMode::Write {
        apply_migration_entries(
            &plan.entries,
            &plan.cleanup_only_sources,
            &options.cleanup,
        )?;
    }

    Ok(MigrateResult {
        entries: plan.entries,
        cleanup_only_sources: plan.cleanup_only_sources,
        warnings: plan.warnings,
    })
}

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::config::{
    Env, RYL_USER_GLOBAL_CONFIG_CANDIDATES, SystemEnv, YamlLintConfig, is_toml_path,
    legacy_yaml,
};
use crate::config_schema::{parse_toml_config_str, toml_config_to_value};
use crate::rules::{comments, document_end, document_start, quoted_strings};

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

/// A yamllint user-global config to migrate to ryl's own user-global location, whose
/// existing ryl TOML configs are also rewritten if they set a deprecated key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserConfigMigration {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl UserConfigMigration {
    /// The existing ryl user-global TOML configs beside `target`.
    #[must_use]
    pub fn ryl_config_paths(&self) -> Vec<PathBuf> {
        let dir = self.target.parent().unwrap_or(Path::new(""));
        RYL_USER_GLOBAL_CONFIG_CANDIDATES
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .collect()
    }
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

/// One config to write. A `source` equal to its `target` is a ryl TOML config rewritten
/// in place: `RenameSuffix` cleanup copies it to the backup name before the write, and
/// `Delete` leaves it alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEntry {
    pub source: PathBuf,
    pub target: PathBuf,
    pub toml: String,
}

impl MigrationEntry {
    fn is_rewrite(&self) -> bool {
        self.source == self.target
    }
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
    // Symlinked sources are excluded: cleanup never renames them.
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
        // Never delete or rename through a symlink: acting on the target would orphan the
        // real file, so a symlinked source is preserved (mirrors the --fix/--diff refusal).
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
        // The user-global target dir (e.g. ~/.config/ryl/) may not exist yet. A target with
        // no parent (never produced by the planner) skips this and lets the write below
        // report any failure, so this can never panic.
        if let Some(parent) = entry.target.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create directory {} for migrated config: {err}",
                    parent.display()
                )
            })?;
        }
        if let SourceCleanup::RenameSuffix(suffix) = cleanup
            && entry.is_rewrite()
        {
            let backup = rename_destination(&entry.source, suffix);
            fs::read(&entry.source)
                .and_then(|original| {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&backup)
                        .and_then(|mut file| file.write_all(&original))
                })
                .map_err(|err| {
                    format!(
                        "failed to back up {} to {}: {err}",
                        entry.source.display(),
                        backup.display()
                    )
                })?;
        }
        // `create_new` atomically refuses (without following a symlink) to overwrite an
        // existing target, so a target appearing between planning and writing can never be
        // clobbered by a stale plan.
        fs::OpenOptions::new()
            .write(true)
            .create_new(!entry.is_rewrite())
            .truncate(entry.is_rewrite())
            .open(&entry.target)
            .and_then(|mut file| file.write_all(entry.toml.as_bytes()))
            .map_err(|err| {
                format!(
                    "failed to write migrated config {}: {err}",
                    entry.target.display()
                )
            })?;
    }

    for source in cleanup_sources(entries, cleanup_only_sources) {
        apply_cleanup(source)?;
    }

    Ok(())
}

fn cleanup_sources<'a>(
    entries: &'a [MigrationEntry],
    cleanup_only_sources: &'a [PathBuf],
) -> impl Iterator<Item = &'a Path> {
    entries
        .iter()
        .filter(|entry| !entry.is_rewrite())
        .map(|entry| entry.source.as_path())
        .chain(cleanup_only_sources.iter().map(PathBuf::as_path))
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

fn is_ryl_toml_config_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".ryl.toml" | "ryl.toml" | "pyproject.toml"))
}

fn discover_configs(root: &Path, is_config: fn(&Path) -> bool) -> Vec<PathBuf> {
    if root.is_file() {
        return if is_config(root) {
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
        .filter(|path| path.is_file() && is_config(path))
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

/// An existing ryl-native TOML config *file* that a migration into `target` would
/// overwrite or be shadowed by. For a discovery name (`.ryl.toml`/`ryl.toml`) that is any
/// root name or `.config/` candidate in its directory, which a written `<dir>/.ryl.toml`
/// would silently outrank; any other target is only ever named by `-c`, so only the
/// target itself collides. A non-file (e.g. a directory) is not a collision, leaving the
/// write path to report it.
fn existing_ryl_native_config(target: &Path) -> Option<PathBuf> {
    if !is_ryl_toml_config_path(target) {
        return target.is_file().then(|| target.to_path_buf());
    }
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

/// Convert one YAML config at `source` into a TOML `MigrationEntry` written to `target`.
/// Skips (with a warning) when the source or target is a symlink, or a ryl-native config
/// already exists at the target. Returns `true` when an entry was added, `false` when
/// skipped, so callers avoid cleaning up sources for a directory that was not migrated.
///
/// `user_global` marks the user-global config, whose target moves to `<config-dir>/ryl/`,
/// so a relative top-level `ignore-from-file` is inlined to keep it from dangling and a
/// relative rule-level one (not relocatable without rewriting the rule config) is refused.
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
    let base_dir = if user_global {
        SystemEnv.current_dir()
    } else {
        source.parent().unwrap_or(Path::new("")).to_path_buf()
    };
    let mut config = legacy_yaml::load(&SystemEnv, source, &base_dir)?;
    // At runtime a user-global relative path resolves from each linted file's directory,
    // which no single migrated file can express.
    if user_global && let Some(path) = config.relative_ignore_from_file() {
        plan.warnings.push(format!(
            "warning: skipping migration of {}: its relative ignore-from-file `{path}` \
             resolves against each linted file's directory, which the ryl user-global \
             config cannot express; make the path absolute or move the setting into the \
             project config, then re-run",
            source.display()
        ));
        return Ok(false);
    }
    config.finalize(&SystemEnv, &base_dir)?;
    if user_global {
        config.inline_resolved_ignore_from_file();
    }
    if !config.enables_any_rule() {
        plan.warnings.push(format!(
            "warning: migrated config {} enables no rules; ryl will not lint with it \
             \u{2014} enable at least one rule, or use 'extends: default' for the standard rule set",
            target.display()
        ));
    }
    let rendered = config.to_toml_string();
    let mut toml = format!("{}\n", rendered.trim_end());
    let targets = preserve_targets(&config);
    if !targets.is_empty() {
        let table = toml::Table::from_iter([("format".to_owned(), targets.into())]);
        toml.push('\n');
        toml.push_str(&toml::to_string(&table).expect("a string table serializes"));
    }
    plan.entries.push(MigrationEntry {
        source: source.to_path_buf(),
        target,
        toml,
    });
    Ok(true)
}

/// `preserve` for each `[format]` target whose rule `cfg` disables, or whose enforcing
/// option it turns off, so `ryl format` leaves alone what the legacy config never enforced.
fn preserve_targets(cfg: &YamlLintConfig) -> toml::Table {
    [
        (document_start::ID, Some("present"), "document-start"),
        (document_end::ID, Some("present"), "document-end"),
        (quoted_strings::ID, None, "quote-style"),
        (
            comments::ID,
            Some("require-starting-space"),
            "comment-starting-space",
        ),
    ]
    .into_iter()
    .filter(|(rule, option, _)| {
        cfg.rule_level(rule).is_none()
            || option.is_some_and(|option| !cfg.rule_option_bool(rule, option, true))
    })
    .map(|(_, _, key)| (key.to_owned(), "preserve".into()))
    .collect()
}

/// The legacy configs under `root`. A file root is taken whatever its name, unless it is
/// TOML, because `-c` loads any such file as yamllint config.
fn legacy_yaml_sources(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return (!is_toml_path(root))
            .then(|| root.to_path_buf())
            .into_iter()
            .collect();
    }
    discover_configs(root, is_legacy_yaml_config_path)
}

fn build_project_entries(root: &Path, plan: &mut MigrationPlan) -> Result<(), String> {
    let mut grouped: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for path in legacy_yaml_sources(root) {
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
        // Enqueue lower-precedence siblings for cleanup only once the primary migrated,
        // else a skipped directory would still delete/rename them with --delete-old/--rename-old.
        let target = if is_legacy_yaml_config_path(&primary) {
            dir.join(".ryl.toml")
        } else {
            primary.with_extension("toml")
        };
        if build_entry(&primary, target, plan, false)? {
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

/// Plan an in-place rewrite of each ryl TOML config under `root` that sets a deprecated
/// key.
fn build_toml_rewrites(root: &Path, plan: &mut MigrationPlan) {
    let mut paths = discover_configs(root, is_ryl_toml_config_path);
    paths.sort();
    for path in paths {
        plan_toml_rewrite(path, plan);
    }
}

/// Plan an in-place rewrite of the ryl TOML config at `path` if it sets a deprecated key.
/// `pyproject.toml` is only reported: re-serialising it would drop the comments and
/// layout of every other tool's settings. A file that fails to load is skipped with a
/// warning, so a stray fixture cannot block the rest of the migration.
fn plan_toml_rewrite(path: PathBuf, plan: &mut MigrationPlan) {
    if plan
        .entries
        .iter()
        .any(|entry| same_file::is_same_file(&entry.source, &path).unwrap_or(false))
    {
        return;
    }
    let pyproject = path
        .file_name()
        .is_some_and(|name| name == "pyproject.toml");
    let loaded = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read config: {err}"))
        .and_then(|text| parse_toml_config_str(&text, pyproject));
    let config = match loaded {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(err) => {
            plan.warnings
                .push(format!("warning: skipping {}: {err}", path.display()));
            return;
        }
    };
    let deprecated = config.deprecated_keys();
    if deprecated.is_empty() {
        return;
    }
    if is_symlink(&path) {
        plan.warnings.push(format!(
            "warning: skipping {}: refusing to follow a symlink",
            path.display()
        ));
        return;
    }
    if pyproject {
        let moves = deprecated
            .iter()
            .map(|used| format!("`{}` to `{}`", used.key.key, used.key.replacement))
            .collect::<Vec<_>>()
            .join(", ");
        plan.warnings.push(format!(
            "warning: not rewriting {}: in [tool.ryl], move {moves}",
            path.display()
        ));
        return;
    }
    let rendered = toml::to_string_pretty(&toml_config_to_value(&config.to_nested()))
        .expect("serializing a TOML value cannot fail");
    plan.entries.push(MigrationEntry {
        source: path.clone(),
        target: path,
        toml: format!("{}\n", rendered.trim_end()),
    });
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
        build_toml_rewrites(root, &mut plan);
    }
    if let Some(user) = &options.user_config {
        for path in user.ryl_config_paths() {
            plan_toml_rewrite(path, &mut plan);
        }
        if user.source.exists() {
            build_entry(&user.source, user.target.clone(), &mut plan, true)?;
        }
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

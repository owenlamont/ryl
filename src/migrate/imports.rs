use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::config::{Env, SystemEnv, YamlLintConfig};
use crate::config_schema::RuleLevel;
use crate::yaml_dom::YamlOwned;

use super::{
    MigrationEntry, MigrationPlan, SourceCleanup,
    absolute_source_path as absolute_source, resolved_source_path,
};

mod bounded_json5;
mod discovery;
mod dprint;
mod options;
mod prettier;
mod read;
mod yamlfix;
mod yamlfmt;

use discovery::Candidate;
use options::{Converted, escaped};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[non_exhaustive]
pub enum MigrationSource {
    Yamllint,
    Prettier,
    Yamlfmt,
    #[value(alias = "pretty-yaml", alias = "pretty_yaml")]
    Dprint,
    Yamlfix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Formatter {
    Prettier,
    Yamlfmt,
    Dprint,
    Yamlfix,
}

impl Formatter {
    fn selection(self) -> MigrationSource {
        match self {
            Self::Prettier => MigrationSource::Prettier,
            Self::Yamlfmt => MigrationSource::Yamlfmt,
            Self::Dprint => MigrationSource::Dprint,
            Self::Yamlfix => MigrationSource::Yamlfix,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Prettier => "Prettier",
            Self::Yamlfmt => "yamlfmt",
            Self::Dprint => "dprint pretty_yaml",
            Self::Yamlfix => "yamlfix",
        }
    }
}

#[derive(Default)]
struct Directory {
    lint: Vec<PathBuf>,
    format: Vec<PathBuf>,
}

pub(super) fn build_project_entries(
    root: &Path,
    selection: Option<MigrationSource>,
    cleanup: &SourceCleanup,
    plan: &mut MigrationPlan,
) -> Result<(), String> {
    if selection == Some(MigrationSource::Yamllint) {
        return super::build_legacy_project_entries(root, plan);
    }
    let explicit = root.is_file();
    let format_files = if explicit && selection.is_some() {
        vec![root.to_path_buf()]
    } else {
        super::discover_configs(root, discovery::is_source)
    };
    if format_files.is_empty() {
        return super::build_legacy_project_entries(root, plan);
    }
    let mut directories: BTreeMap<PathBuf, Directory> = BTreeMap::new();
    let lint_sources = if explicit {
        super::YAML_CONFIG_NAMES
            .iter()
            .map(|name| parent(root).join(name))
            .filter(|path| path.is_file())
            .collect()
    } else {
        super::legacy_yaml_sources(root)
    };
    for path in lint_sources {
        directories
            .entry(parent(&path))
            .or_default()
            .lint
            .push(path);
    }
    for path in format_files {
        directories
            .entry(parent(&path))
            .or_default()
            .format
            .push(path);
    }
    for (directory, files) in directories {
        let directory_plan =
            build_directory(&directory, files, root, selection, cleanup)?;
        plan.entries.extend(directory_plan.entries);
        plan.cleanup_only_sources
            .extend(directory_plan.cleanup_only_sources);
        plan.warnings.extend(directory_plan.warnings);
    }
    Ok(())
}

fn select_candidate(
    format_paths: &[PathBuf],
    explicit: bool,
    selection: Option<MigrationSource>,
    directory: &Path,
) -> Result<Option<Candidate>, String> {
    let mut candidates = Vec::new();
    for tool in [
        Formatter::Prettier,
        Formatter::Yamlfmt,
        Formatter::Dprint,
        Formatter::Yamlfix,
    ] {
        if selection.is_some_and(|selected| selected != tool.selection()) {
            continue;
        }
        let paths: Vec<_> = format_paths
            .iter()
            .filter(|path| {
                explicit && selection == Some(tool.selection())
                    || discovery::kind(path) == Some(tool)
            })
            .cloned()
            .collect();
        if let Some(candidate) = discovery::load(tool, &paths, explicit)? {
            candidates.push(candidate);
        }
    }
    if candidates.len() > 1 {
        let names = candidates
            .iter()
            .map(|candidate| candidate.tool.name())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "multiple YAML formatter configurations apply in {} ({names}); select one with --migrate-from",
            directory.display()
        ));
    }
    Ok(candidates.pop())
}

fn convert(candidate: &mut Candidate) -> Result<Option<Converted>, String> {
    let Some(values) = candidate.values.take() else {
        return Ok(None);
    };
    let converted = match candidate.tool {
        Formatter::Prettier => prettier::convert(values),
        Formatter::Yamlfmt => yamlfmt::convert(values),
        Formatter::Dprint => dprint::convert(values),
        Formatter::Yamlfix => yamlfix::convert(values),
    };
    converted.map(Some).map_err(|error| {
        format!(
            "invalid config {}: {error}",
            escaped(&candidate.source.display().to_string())
        )
    })
}

fn build_directory(
    directory: &Path,
    mut files: Directory,
    root: &Path,
    selection: Option<MigrationSource>,
    cleanup: &SourceCleanup,
) -> Result<MigrationPlan, String> {
    let mut plan = MigrationPlan::default();
    files
        .lint
        .sort_by_key(|path| (super::yaml_config_rank(path), path.clone()));
    let target = directory.join(".ryl.toml");
    let primary = files
        .lint
        .first()
        .or_else(|| files.format.first())
        .expect("a discovered directory contains a source");
    if !files.lint.is_empty()
        && !build_lint_entry(&files.lint, root, &target, cleanup, &mut plan)?
    {
        return Ok(plan);
    }
    if files.lint.is_empty() {
        if super::is_symlink(&target) {
            plan.warnings.push(format!(
                "warning: skipping migration to {}: refusing to follow a symlink",
                target.display()
            ));
            return Ok(plan);
        }
        if let Some(existing) = super::existing_ryl_native_config(&target, false)? {
            plan.warnings.push(format!("warning: skipping migration of {}: a ryl-native config already exists at {}", primary.display(), existing.display()));
            return Ok(plan);
        }
    }
    let mut candidate =
        select_candidate(&files.format, root.is_file(), selection, directory)?;
    let mut converted = match &mut candidate {
        Some(candidate) => convert(candidate)?,
        None => None,
    };
    if root.is_file() && converted.is_none() {
        plan = MigrationPlan::default();
    }
    let imported = converted.is_some();
    let cleanup_safe = converted
        .as_ref()
        .is_some_and(|converted| converted.cleanup_safe);
    let mut warnings = candidate
        .as_ref()
        .map_or_else(Vec::new, |candidate| candidate.warnings.clone());
    if let Some(converted) = &mut converted {
        warnings.append(&mut converted.warnings);
    }
    if files.lint.is_empty() && converted.is_some() {
        let candidate = candidate
            .as_ref()
            .expect("converted settings have a source");
        if let Some(inherited) = inherited_lint(directory) {
            plan.warnings.push(discovery::warning(&candidate.source, &format!("formatter-only import would shadow inherited configuration {}; source retained; combine lint and formatter settings in the ancestor ryl config manually", escaped(&inherited.display().to_string()))));
            return Ok(plan);
        }
        let cleanup_source =
            cleanup_safe && cleanup_allowed(candidate, &candidate.source, root);
        plan.entries.push(MigrationEntry {
            source: candidate.source.clone(),
            cleanup_source,
            target,
            toml: String::new(),
        });
    }
    if let Some(candidate) = &candidate {
        if *cleanup != SourceCleanup::Keep {
            for source in &candidate.sources {
                if !cleanup_safe || !cleanup_allowed(candidate, source, root) {
                    warnings.push(format!("source cleanup skipped for {}; shared, unrecognized, inherited-outside-root or unimported configurations are retained", escaped(&source.display().to_string())));
                }
            }
        }
        if let Some(entry) = plan.entries.first_mut() {
            entry.toml = compose(&entry.toml, converted, candidate, &warnings);
            for source in &candidate.sources {
                if imported
                    && cleanup_safe
                    && cleanup_allowed(candidate, source, root)
                    && entry.source != *source
                {
                    plan.cleanup_only_sources.push(source.clone());
                }
            }
        }
        plan.warnings.extend(
            warnings
                .iter()
                .map(|warning| discovery::warning(&candidate.source, warning)),
        );
    }
    Ok(plan)
}

fn build_lint_entry(
    files: &[PathBuf],
    root: &Path,
    target: &Path,
    cleanup: &SourceCleanup,
    plan: &mut MigrationPlan,
) -> Result<bool, String> {
    let lint = files.first().expect("lint sources were discovered");
    if !super::build_entry(lint, target.to_path_buf(), plan, false)? {
        return Ok(false);
    }
    if root.is_file() {
        plan.entries
            .last_mut()
            .expect("the legacy entry was just constructed")
            .cleanup_source = false;
        if *cleanup != SourceCleanup::Keep {
            for source in files {
                plan.warnings.push(discovery::warning(source, "source cleanup skipped: colocated lint is retained outside the selected file cleanup scope"));
            }
        }
    } else {
        plan.cleanup_only_sources
            .extend(files.iter().skip(1).cloned());
    }
    for source in files.iter().skip(1) {
        plan.warnings.push(format!(
            "warning: skipping lower-precedence config {} in favor of {}",
            source.display(),
            lint.display()
        ));
    }
    Ok(true)
}

fn compose(
    legacy: &str,
    converted: Option<Converted>,
    candidate: &Candidate,
    warnings: &[String],
) -> String {
    let mut table = if legacy.is_empty() {
        toml::Table::new()
    } else {
        toml::from_str::<toml::Table>(legacy)
            .expect("the legacy converter writes valid TOML")
    };
    if let Some(converted) = converted {
        if !legacy.is_empty() {
            let before = YamlLintConfig::from_toml_str(legacy)
                .expect("the legacy converter writes loadable TOML");
            if converted.width.is_some() {
                let max = before.rule_option_int("line-length", "max", 80);
                pin_lint(&mut table, &before, "line-length", "max", max.into());
            }
            if converted.indent.is_some() {
                let spaces = lint_spaces(&before);
                pin_lint(&mut table, &before, "indentation", "spaces", spaces);
            }
        }
        if let Some(indent) = converted.indent {
            table.insert("indent-width".to_owned(), i64::from(indent).into());
        }
        if let Some(width) = converted.width {
            table.insert("line-length".to_owned(), i64::from(width).into());
        }
        table.insert(
            "format".to_owned(),
            toml::Value::try_from(converted.targets)
                .expect("typed formatter settings serialize"),
        );
    }
    let body = toml::to_string_pretty(&table).expect("a TOML table serializes");
    let mut result = format!(
        "# Migration source: {} ({})\n",
        escaped(&candidate.source.display().to_string()),
        candidate.tool.name()
    );
    for warning in warnings {
        writeln!(result, "# Migration warning: {}", escaped(warning))
            .expect("writing to a String cannot fail");
    }
    result.push_str(&body);
    result
}

fn lint_spaces(config: &YamlLintConfig) -> toml::Value {
    config
        .rule_option("indentation", "spaces")
        .and_then(YamlOwned::as_integer)
        .map_or_else(|| "consistent".into(), toml::Value::from)
}

fn pin_lint(
    table: &mut toml::Table,
    config: &YamlLintConfig,
    rule: &str,
    option: &str,
    value: toml::Value,
) {
    let Some(level) = config.rule_level(rule) else {
        return;
    };
    let rules = table
        .get_mut("lint")
        .and_then(toml::Value::as_table_mut)
        .and_then(|lint| lint.get_mut("rules"))
        .and_then(toml::Value::as_table_mut)
        .expect("the legacy converter writes enabled rules in a lint table");
    let mut options = match rules
        .remove(rule)
        .expect("the legacy converter writes each enabled rule")
    {
        toml::Value::Table(options) => options,
        _ => toml::Table::new(),
    };
    options.insert(
        "level".to_owned(),
        match level {
            RuleLevel::Error => "error",
            RuleLevel::Warning => "warning",
        }
        .into(),
    );
    options.insert(option.to_owned(), value);
    rules.insert(rule.to_owned(), options.into());
}

fn cleanup_allowed(candidate: &Candidate, source: &Path, root: &Path) -> bool {
    discovery::can_cleanup(candidate.tool, source)
        && resolved_source_path(source).starts_with(resolved_source_path(root))
}

fn parent(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

fn inherited_lint(directory: &Path) -> Option<PathBuf> {
    let directory = resolved_source_path(directory);
    let home = SystemEnv
        .env_var("HOME")
        .map(PathBuf::from)
        .or_else(dirs_next::home_dir)
        .map(|path| resolved_source_path(&path));
    if home.as_deref() == Some(directory.as_path()) {
        return None;
    }
    for ancestor in directory.parent().into_iter().flat_map(Path::ancestors) {
        if let Some(source) = super::YAML_CONFIG_NAMES
            .iter()
            .map(|name| ancestor.join(name))
            .find(|path| path.is_file())
        {
            return Some(source);
        }
        if home.as_deref() == Some(ancestor) {
            break;
        }
    }
    None
}

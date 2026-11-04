use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::config::{SourceKind, YamlLintConfig};
use crate::directives::{FormatterComments, migrate_source_comments};

use super::{MigrateOptions, MigrateResult, MigrationSource, WriteMode, imports};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot read {}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot discover application files under {}", path.display())]
    Discover {
        path: PathBuf,
        source: ignore::Error,
    },
    #[error("invalid migration configuration: {0}")]
    Config(String),
    #[error("refusing to overwrite changed or symlinked application file {}", .0.display())]
    Changed(PathBuf),
    #[error("cannot write migrated comments in {}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub struct DirectiveFile {
    pub path: PathBuf,
    pub diff: Option<String>,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

pub struct DirectiveMigration {
    pub config: MigrateResult,
    pub files: Vec<DirectiveFile>,
    pub warnings: Vec<String>,
}

struct Profile {
    source: Option<MigrationSource>,
    marker: String,
    yaml: bool,
    markdown: bool,
    infer_prettier_parser: bool,
    paths: Vec<PathBuf>,
}

impl Profile {
    fn comments(&self) -> FormatterComments<'_> {
        match self.source {
            Some(MigrationSource::Prettier) => FormatterComments::Prettier,
            Some(MigrationSource::Yamlfmt) => FormatterComments::Yamlfmt,
            Some(MigrationSource::Dprint) => {
                FormatterComments::PrettyYaml(&self.marker)
            }
            _ => FormatterComments::None,
        }
    }

    fn load(
        directory: &Path,
        selection: Option<MigrationSource>,
        warnings: &mut Vec<String>,
    ) -> Result<Self, Error> {
        let context =
            imports::source_context(directory, selection).map_err(Error::Config)?;
        let source = context.as_ref().map(|context| context.source).or(selection);
        let mut profile = Self {
            source,
            marker: "pretty-yaml-ignore".to_owned(),
            yaml: true,
            markdown: false,
            infer_prettier_parser: false,
            paths: Vec::new(),
        };
        let Some(context) = context else {
            profile.yaml = source != Some(MigrationSource::Dprint);
            profile.markdown = source == Some(MigrationSource::Prettier);
            profile.infer_prettier_parser = source == Some(MigrationSource::Prettier);
            return Ok(profile);
        };
        profile.paths = context.paths;
        warnings.extend(context.warnings);
        let Some(values) = context.values else {
            profile.yaml = false;
            return Ok(profile);
        };
        match source {
            Some(MigrationSource::Prettier) => profile.prettier(&values, warnings),
            Some(MigrationSource::Dprint) => {
                if let Some(marker) = values
                    .get("yaml")
                    .and_then(|value| value.get("ignoreCommentDirective"))
                    .and_then(Value::as_str)
                {
                    marker.clone_into(&mut profile.marker);
                }
                profile.yaml = audited_pretty_yaml_plugin(&values)
                    && !values.contains_key("extends")
                    && values
                        .get("yaml")
                        .is_none_or(|yaml| yaml.get("associations").is_none())
                    && !profile.marker.is_empty()
                    && profile.marker.trim() == profile.marker;
                if !profile.yaml {
                    warnings.push("dprint plugin, inheritance, association or ignore-marker settings prevent proving formatter comment scope; source markers retained".to_owned());
                }
            }
            _ => {}
        }
        Ok(profile)
    }

    fn prettier(&mut self, values: &Map<String, Value>, warnings: &mut Vec<String>) {
        let unsupported = values.contains_key("overrides")
            || values.contains_key("plugins")
            || values.get("requirePragma") == Some(&Value::Bool(true))
            || values.get("checkIgnorePragma") == Some(&Value::Bool(true));
        let parser = values.get("parser").and_then(Value::as_str);
        self.infer_prettier_parser = parser.is_none();
        self.yaml = !unsupported && parser.is_none_or(|parser| parser == "yaml");
        self.markdown = !unsupported
            && parser.is_none_or(|parser| parser == "markdown")
            && values
                .get("embeddedLanguageFormatting")
                .and_then(Value::as_str)
                != Some("off");
        if unsupported || !self.yaml && !self.markdown {
            warnings.push("Prettier per-file, plugin, parser or pragma settings prevent proving formatter comment scope; source markers retained".to_owned());
        }
    }
}

fn audited_pretty_yaml_plugin(values: &Map<String, Value>) -> bool {
    const PLUGIN: &str = "https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm";
    values
        .get("plugins")
        .and_then(Value::as_array)
        .is_some_and(|plugins| {
            plugins.iter().filter_map(Value::as_str).any(|plugin| {
                if plugin == "npm:dprint-plugin-yaml@0.6.0" {
                    return true;
                }
                let (url, checksum) = plugin
                    .split_once('@')
                    .map_or((plugin, None), |(url, checksum)| (url, Some(checksum)));
                url == PLUGIN
                    && checksum.is_none_or(|checksum| {
                        checksum.len() == 64
                            && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
            })
        })
}

#[derive(Default)]
struct ApplicationFiles {
    files: Vec<(PathBuf, SourceKind)>,
    configs: Vec<PathBuf>,
}

fn application_files(root: &Path) -> Result<ApplicationFiles, Error> {
    let base = if root.is_dir() {
        root
    } else {
        root.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    };
    let mut config = YamlLintConfig::default();
    config.enable_default_markdown(base);
    let paths = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        let mut paths = Vec::new();
        for entry in crate::discover::walk_dir(root) {
            let entry = entry.map_err(|source| Error::Discover {
                path: root.to_path_buf(),
                source,
            })?;
            if crate::discover::is_walked_file(&entry) {
                paths.push(entry.into_path());
            }
        }
        paths
    };
    let mut files = Vec::new();
    let mut configs = Vec::new();
    for path in paths {
        if imports::is_source_config(&path) {
            configs.push(path);
        } else if let Some(kind) = config
            .source_kind(&path, base)
            .expect("default YAML and Markdown source patterns are disjoint")
        {
            files.push((path, kind));
        }
    }
    files.sort_by(|(a, _), (b, _)| a.cmp(b));
    files.dedup_by(|(a, _), (b, _)| a == b);
    Ok(ApplicationFiles { files, configs })
}

fn unknown_marker_lines(content: &str) -> Vec<usize> {
    if !content.contains("ignore") {
        return Vec::new();
    }
    let version = crate::text_version::TextVersion::new(content);
    crate::rules::support::comments_scan::collect_comments_with_analysis(
        version.analysis(),
    )
    .into_iter()
    .filter(|comment| {
        matches!(
            comment.text.trim_matches([' ', '\t']),
            "prettier-ignore" | "pretty-yaml-ignore" | "!yamlfmt!:ignore"
        )
    })
    .map(|comment| comment.span.start.line())
    .collect()
}

fn plan_file(
    path: PathBuf,
    kind: SourceKind,
    profile: &Profile,
    protected: &[PathBuf],
    warnings: &mut Vec<String>,
) -> Result<Option<DirectiveFile>, Error> {
    if super::is_symlink(&path) {
        warnings.push(format!("{}: symlink left unchanged", path.display()));
        return Ok(None);
    }
    if let Some(identity) = crate::fix::file_identity(&path)
        && profile.paths.iter().chain(protected).any(|source| {
            crate::fix::file_identity(source).is_some_and(|other| other == identity)
        })
    {
        return Ok(None);
    }
    let original = fs::read(&path).map_err(|source| Error::Read {
        path: path.clone(),
        source,
    })?;
    let decoded = match crate::decoder::decode_bytes_lossless(&original) {
        Ok(decoded) => decoded,
        Err(error) => {
            warnings.push(format!("{}: {error}; comments retained", path.display()));
            return Ok(None);
        }
    };
    let content = decoded.content();
    if decoded.encode(content) != original {
        warnings.push(format!(
            "{}: encoding does not round-trip exactly; comments retained",
            path.display()
        ));
        return Ok(None);
    }
    if kind == SourceKind::Markdown
        && crate::markdown_embed::markdown_has_unsupported_cr(content)
    {
        warnings.push(format!("{}: bare carriage return prevents extracting embedded YAML; comments retained", path.display()));
        return Ok(None);
    }
    let known_parser = !profile.infer_prettier_parser
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                ["yaml", "yml", "md", "markdown", "mdx"]
                    .iter()
                    .any(|known| extension.eq_ignore_ascii_case(known))
            });
    let permit = known_parser
        && if kind == SourceKind::Yaml {
            profile.yaml
        } else {
            profile.markdown
        };
    if profile.source.is_none() {
        let lines = match kind {
            SourceKind::Yaml => unknown_marker_lines(content),
            SourceKind::Markdown => crate::markdown_embed::extract_regions(
                content,
                crate::markdown_embed::MarkdownSources {
                    front_matter: true,
                    fenced_blocks: true,
                },
            )
            .into_iter()
            .flat_map(|region| {
                unknown_marker_lines(&region.content)
                    .into_iter()
                    .map(move |line| line + region.line_offset)
            })
            .collect(),
        };
        for line in lines {
            warnings.push(format!("{}:{line}: formatter directive retained; select its source with --migrate-from", path.display()));
        }
    }
    let migration = migrate_source_comments(content, profile.comments(), kind, permit);
    for line in migration.retained_lines {
        warnings.push(format!("{}:{line}: formatter directive retained because equivalent native scope is not proven", path.display()));
    }
    let Some(output) = migration.output else {
        return Ok(None);
    };
    let replacement = decoded.encode(&output);
    let applicable = decoded.is_plain_utf8()
        && !crate::fix::path_unrepresentable_in_diff(&path)
        && !content.ends_with('\r')
        && !output.ends_with('\r');
    let diff = applicable
        .then(|| crate::fix::render_unified_diff(content, &output, &path))
        .flatten();
    if diff.is_none() {
        warnings.push(format!("{}: no applicable text diff; use --migrate-write to apply comment conversion", path.display()));
    }
    Ok(Some(DirectiveFile {
        path,
        diff,
        original,
        replacement,
    }))
}

/// # Errors
/// Returns an error if a source changed, became a symlink or cannot be written.
pub fn apply_directive_entries(files: &[DirectiveFile]) -> Result<(), Error> {
    for file in files {
        let metadata =
            fs::symlink_metadata(&file.path).map_err(|source| Error::Read {
                path: file.path.clone(),
                source,
            })?;
        if !metadata.file_type().is_file() {
            return Err(Error::Changed(file.path.clone()));
        }
        let current = fs::read(&file.path).map_err(|source| Error::Read {
            path: file.path.clone(),
            source,
        })?;
        if current != file.original {
            return Err(Error::Changed(file.path.clone()));
        }
    }
    for file in files {
        crate::atomic_write::write(&file.path, &file.replacement, true).map_err(
            |source| Error::Write {
                path: file.path.clone(),
                source,
            },
        )?;
    }
    Ok(())
}

/// # Errors
/// Returns an error if discovering, planning, writing or cleaning up a migration fails.
pub fn migrate_with_directives(
    options: &MigrateOptions,
    root: &Path,
) -> Result<DirectiveMigration, Error> {
    if !root.exists() {
        return Err(Error::Config(format!(
            "migrate root does not exist: {}",
            root.display()
        )));
    }
    let mut preview = options.clone();
    preview.write_mode = WriteMode::Preview;
    let config = super::migrate_configs(&preview).map_err(Error::Config)?;
    let mut warnings = Vec::new();
    let mut profiles = BTreeMap::new();
    let mut files = Vec::new();
    let explicit_source = options
        .project_root
        .as_ref()
        .filter(|source| source.is_file());
    let mut discovered = if explicit_source.is_some_and(|source| {
        super::resolved_source_path(source) == super::resolved_source_path(root)
    }) {
        ApplicationFiles::default()
    } else {
        application_files(root)?
    };
    if let Some(source) = explicit_source {
        discovered.configs.push(source.clone());
    }
    if let Some(user) = &options.user_config {
        discovered
            .configs
            .extend([user.source.clone(), user.target.clone()]);
        discovered.configs.extend(user.ryl_config_paths());
    }
    discovered.configs.extend(
        config
            .entries
            .iter()
            .flat_map(|entry| [&entry.source, &entry.target])
            .cloned(),
    );
    discovered
        .configs
        .extend(config.cleanup_only_sources.iter().cloned());
    for (path, kind) in discovered.files {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        if !profiles.contains_key(&directory) {
            profiles.insert(
                directory.clone(),
                Profile::load(&directory, options.from, &mut warnings)?,
            );
        }
        if let Some(file) = plan_file(
            path,
            kind,
            &profiles[&directory],
            &discovered.configs,
            &mut warnings,
        )? {
            files.push(file);
        }
    }
    if options.write_mode == WriteMode::Write {
        super::preflight_entries(
            &config.entries,
            &config.cleanup_only_sources,
            &options.cleanup,
        )
        .map_err(Error::Config)?;
        super::write_entries(&config.entries, &options.cleanup)
            .map_err(Error::Config)?;
        apply_directive_entries(&files)?;
        super::cleanup_entries(
            &config.entries,
            &config.cleanup_only_sources,
            &options.cleanup,
        )
        .map_err(Error::Config)?;
    }
    warnings.sort();
    warnings.dedup();
    Ok(DirectiveMigration {
        config,
        files,
        warnings,
    })
}

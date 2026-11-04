use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{
    Formatter, absolute_source,
    options::{escaped, truthy},
    read, resolved_source_path,
};

const PRETTIER: &[&str] = &[
    "package.json",
    "package.yaml",
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.yml",
    ".prettierrc.yaml",
    ".prettierrc.json5",
    ".prettierrc.js",
    "prettier.config.js",
    ".prettierrc.ts",
    "prettier.config.ts",
    ".prettierrc.mjs",
    "prettier.config.mjs",
    ".prettierrc.mts",
    "prettier.config.mts",
    ".prettierrc.cjs",
    "prettier.config.cjs",
    ".prettierrc.cts",
    "prettier.config.cts",
    ".prettierrc.toml",
];
const YAMLFMT: &[&str] = &[
    ".yamlfmt",
    "yamlfmt.yml",
    "yamlfmt.yaml",
    ".yamlfmt.yaml",
    ".yamlfmt.yml",
];
const DPRINT: &[&str] = &[
    "dprint.json",
    "dprint.jsonc",
    ".dprint.json",
    ".dprint.jsonc",
];
const YAMLFIX: &[&str] = &["pyproject.toml", "yamlfix.toml", ".yamlfix.toml"];

pub(super) struct Candidate {
    pub tool: Formatter,
    pub source: PathBuf,
    pub sources: Vec<PathBuf>,
    pub values: Option<Map<String, Value>>,
    pub warnings: Vec<String>,
}

impl Candidate {
    fn new(tool: Formatter, source: &Path, values: Map<String, Value>) -> Self {
        let source = absolute_source(source);
        Self {
            tool,
            sources: vec![source.clone()],
            source,
            values: Some(values),
            warnings: Vec::new(),
        }
    }

    fn unsupported(tool: Formatter, source: &Path, reason: &str) -> Self {
        let mut candidate = Self::new(tool, source, Map::new());
        candidate.values = None;
        candidate.warnings.push(reason.to_owned());
        candidate
    }
}

pub(super) fn kind(path: &Path) -> Option<Formatter> {
    let name = path
        .file_name()
        .expect("discovery visits file paths")
        .to_str()?;
    if PRETTIER.contains(&name) {
        Some(Formatter::Prettier)
    } else if YAMLFMT.contains(&name) {
        Some(Formatter::Yamlfmt)
    } else if DPRINT.contains(&name) {
        Some(Formatter::Dprint)
    } else if YAMLFIX.contains(&name) {
        Some(Formatter::Yamlfix)
    } else {
        None
    }
}

pub(super) fn is_source(path: &Path) -> bool {
    if super::super::is_symlink(path) {
        return kind(path).is_some();
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some("pyproject.toml") => std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|value| value.get("tool")?.get("yamlfix").cloned())
            .is_some(),
        Some("package.json" | "package.yaml") => read::prettier(path)
            .ok()
            .and_then(|value| value.get("prettier").cloned())
            .is_some_and(|value| truthy(&value)),
        _ => kind(path).is_some(),
    }
}

pub(super) fn load(
    tool: Formatter,
    paths: &[PathBuf],
    explicit: bool,
) -> Result<Option<Candidate>, String> {
    if paths.is_empty() {
        return Ok(None);
    }
    match tool {
        Formatter::Prettier => prettier(paths),
        Formatter::Yamlfmt => yamlfmt(paths),
        Formatter::Dprint => dprint(paths),
        Formatter::Yamlfix => yamlfix(paths, explicit),
    }
}

fn prettier(paths: &[PathBuf]) -> Result<Option<Candidate>, String> {
    let mut paths = paths.to_vec();
    paths.sort_by_key(|path| rank(path, PRETTIER));
    for path in paths {
        if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("js" | "mjs" | "cjs" | "ts" | "mts" | "cts")
        ) {
            return Ok(Some(Candidate::unsupported(
                Formatter::Prettier,
                &path,
                "JavaScript/TypeScript configurations are not executed; no formatter settings were imported",
            )));
        }
        if super::super::is_symlink(&path) {
            return Ok(Some(Candidate::unsupported(
                Formatter::Prettier,
                &path,
                "refusing to follow a symlink; no formatter settings were imported",
            )));
        }
        let mut value = read::prettier(&path)?;
        if matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("package.json" | "package.yaml")
        ) {
            value = match value.get_mut("prettier") {
                Some(value) if truthy(value) => value.take(),
                _ => continue,
            };
        }
        if let Value::String(name) = &value
            && !name.is_empty()
        {
            return Ok(Some(Candidate::unsupported(
                Formatter::Prettier,
                &path,
                "shared configuration packages are not imported or executed",
            )));
        }
        if value.is_null() || value == false || value == 0 || value == "" {
            value = Value::Object(Map::new());
        }
        if let Value::Array(values) = value {
            value = Value::Object(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), value))
                    .collect(),
            );
        }
        let values = read::object(value, &path)?;
        let mut candidate = Candidate::new(Formatter::Prettier, &path, values);
        let directory = resolved_source_path(path.parent().unwrap_or(Path::new(".")));
        if directory
            .ancestors()
            .any(|parent| parent.join(".editorconfig").is_file())
        {
            candidate.warnings.push("EditorConfig is not imported; unspecified formatter dimensions use Prettier's defaults".to_owned());
        }
        if directory.join(".prettierignore").is_file() {
            candidate
                .warnings
                .push(".prettierignore file selection is not imported".to_owned());
        }
        return Ok(Some(candidate));
    }
    Err("the shared file has no prettier configuration".to_owned())
}

fn yamlfmt(paths: &[PathBuf]) -> Result<Option<Candidate>, String> {
    if paths.len() > 1 {
        return Err("multiple yamlfmt config filenames have nondeterministic source precedence; choose one file with --migrate-root".to_owned());
    }
    let path = &paths[0];
    if super::super::is_symlink(path) {
        return Ok(Some(Candidate::unsupported(
            Formatter::Yamlfmt,
            path,
            "refusing to follow a symlink; no formatter settings were imported",
        )));
    }
    let value = match read::yamlfmt(path)? {
        read::YamlfmtRead::Value(value) => value,
        read::YamlfmtRead::Unsupported(reason) => {
            return Ok(Some(Candidate::unsupported(
                Formatter::Yamlfmt,
                path,
                reason,
            )));
        }
    };
    let values = if value.is_null() {
        Map::new()
    } else {
        read::object(value, path)?
    };
    if let Some(Value::String(kind)) =
        values.get("formatter").and_then(|value| value.get("type"))
        && !kind.is_empty()
        && kind != "basic"
    {
        return Ok(Some(Candidate::unsupported(
            Formatter::Yamlfmt,
            path,
            "only yamlfmt's basic formatter is supported; no formatter settings were imported",
        )));
    }
    Ok(Some(Candidate::new(Formatter::Yamlfmt, path, values)))
}

fn dprint(paths: &[PathBuf]) -> Result<Option<Candidate>, String> {
    let path = paths
        .iter()
        .min_by_key(|path| rank(path, DPRINT))
        .expect("there is at least one candidate");
    if super::super::is_symlink(path) {
        return Ok(Some(Candidate::unsupported(
            Formatter::Dprint,
            path,
            "refusing to follow a symlink; no formatter settings were imported",
        )));
    }
    let values = read::object(read::value(path, read::Syntax::Jsonc)?, path)?;
    let plugin =
        values
            .get("plugins")
            .and_then(Value::as_array)
            .is_some_and(|plugins| {
                plugins
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|plugin| plugin.contains("pretty_yaml"))
            });
    if !plugin && !values.contains_key("yaml") {
        if values.contains_key("extends") {
            return Ok(Some(Candidate::unsupported(
                Formatter::Dprint,
                path,
                "shared dprint configurations are not fetched; no static pretty_yaml settings were found",
            )));
        }
        return Ok(None);
    }
    let mut candidate = Candidate::new(Formatter::Dprint, path, values);
    if !plugin {
        candidate.warnings.push("the pretty_yaml plugin could not be identified statically; importing only the known yaml section and defaults".to_owned());
    }
    Ok(Some(candidate))
}

fn yamlfix(paths: &[PathBuf], explicit: bool) -> Result<Option<Candidate>, String> {
    let local = paths
        .iter()
        .max_by_key(|path| rank(path, YAMLFIX))
        .expect("there is at least one candidate");
    if explicit
        && local
            .file_name()
            .is_some_and(|name| name != "pyproject.toml")
    {
        if super::super::is_symlink(local) {
            return Ok(Some(Candidate::unsupported(
                Formatter::Yamlfix,
                local,
                "refusing to follow a symlink; no formatter settings were imported",
            )));
        }
        if local
            .extension()
            .is_none_or(|extension| extension != "toml")
        {
            return Ok(Some(Candidate::unsupported(
                Formatter::Yamlfix,
                local,
                "only static TOML yamlfix configurations are supported",
            )));
        }
        let values = read::object(read::value(local, read::Syntax::Toml)?, local)
            .expect("TOML documents always contain a table");
        return Ok(Some(Candidate::new(Formatter::Yamlfix, local, values)));
    }
    let directory = resolved_source_path(local.parent().unwrap_or(Path::new(".")));
    let mut merged = Map::new();
    let mut sources = Vec::new();
    let mut local_present = false;
    for name in YAMLFIX {
        let source = if explicit {
            paths
                .iter()
                .find(|path| path.file_name().is_some_and(|file| file == *name))
                .cloned()
        } else {
            directory
                .ancestors()
                .map(|parent| parent.join(name))
                .find(|path| path.is_file())
        };
        let Some(path) = source else {
            continue;
        };
        if super::super::is_symlink(&path) {
            return Ok(Some(Candidate::unsupported(
                Formatter::Yamlfix,
                local,
                "an effective yamlfix source is a symlink; no formatter settings were imported",
            )));
        }
        let mut value = read::value(&path, read::Syntax::Toml)?;
        if *name == "pyproject.toml" {
            let Some(section) = value
                .get_mut("tool")
                .and_then(|tool| tool.get_mut("yamlfix"))
            else {
                continue;
            };
            value = section.take();
        }
        let values = read::object(value, &path)?;
        let absolute = absolute_source(&path);
        local_present |=
            resolved_source_path(path.parent().unwrap_or(Path::new("."))) == directory;
        sources.push(absolute);
        merge(&mut merged, values);
    }
    if !local_present {
        return Ok(None);
    }
    let mut candidate = Candidate::new(Formatter::Yamlfix, local, merged);
    candidate.sources = sources;
    Ok(Some(candidate))
}

fn merge(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(&key), value) {
            (Some(Value::Object(target)), Value::Object(source)) => {
                merge(target, source);
            }
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}

fn rank(path: &Path, names: &[&str]) -> usize {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| names.iter().position(|candidate| *candidate == name))
        .unwrap_or(names.len())
}

pub(super) fn can_cleanup(tool: Formatter, path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    match tool {
        Formatter::Yamlfmt => name.is_some_and(|name| YAMLFMT.contains(&name)),
        Formatter::Yamlfix => matches!(name, Some("yamlfix.toml" | ".yamlfix.toml")),
        Formatter::Prettier | Formatter::Dprint => false,
    }
}

pub(super) fn warning(path: &Path, reason: &str) -> String {
    format!(
        "warning: {}: {}",
        escaped(&path.display().to_string()),
        escaped(reason)
    )
}

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};

use crate::config::{ConfigContext, YamlLintConfig, discover_per_file};

/// Resolve the configuration context for a given file path, optionally using a cached
/// global configuration.
///
/// This mirrors the logic used by the CLI when filtering candidate files.
///
/// # Errors
/// Returns an error when configuration discovery fails for the provided path.
pub fn resolve_ctx<S: BuildHasher>(
    path: &Path,
    global_cfg: Option<&ConfigContext>,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig), S>,
) -> Result<(PathBuf, YamlLintConfig), String> {
    if let Some(gc) = global_cfg {
        return Ok((gc.base_dir.clone(), gc.config.clone()));
    }
    let start = path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    if let Some(pair) = cache.get(&start).cloned() {
        return Ok(pair);
    }
    let ctx = discover_per_file(path)?;
    let pair = (ctx.base_dir.clone(), ctx.config);
    cache.insert(start, pair.clone());
    Ok(pair)
}

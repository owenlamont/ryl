use std::fs;
use std::path::{Path, PathBuf};

use crate::config::YamlLintConfig;
use crate::decoder;
use crate::rules::{comments, new_line_at_end_of_file, new_lines};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixSafety {
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuleFix {
    rule: &'static str,
    safety: FixSafety,
}

const SAFE_FIX_RULES: [RuleFix; 3] = [
    RuleFix {
        rule: new_lines::ID,
        safety: FixSafety::Safe,
    },
    RuleFix {
        rule: comments::ID,
        safety: FixSafety::Safe,
    },
    RuleFix {
        rule: new_line_at_end_of_file::ID,
        safety: FixSafety::Safe,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixStats {
    pub changed_files: usize,
}

/// Apply all currently supported safe fixes to `path` in place.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the fixed contents cannot be written.
pub fn apply_safe_fixes_in_place(
    path: &Path,
    cfg: &YamlLintConfig,
    base_dir: &Path,
) -> Result<bool, String> {
    let original = decoder::read_file(path)?;
    let fixed = apply_safe_fixes(&original, cfg, path, base_dir);
    if fixed == original {
        return Ok(false);
    }

    fs::write(path, fixed).map_err(|err| {
        format!("failed to write fixed file {}: {err}", path.display())
    })?;
    Ok(true)
}

/// Apply all currently supported safe fixes to each discovered file in place.
///
/// # Errors
///
/// Returns an error if any file cannot be read or any fixed contents cannot be written.
pub fn apply_safe_fixes_to_files(
    files: &[(PathBuf, PathBuf, YamlLintConfig)],
) -> Result<(), String> {
    for (path, base_dir, cfg) in files {
        apply_safe_fixes_in_place(path, cfg, base_dir)?;
    }
    Ok(())
}

#[must_use]
pub fn apply_safe_fixes(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> String {
    let mut content = input.to_string();

    if rule_enabled(SAFE_FIX_RULES[0], cfg, path, base_dir) {
        let rule_cfg = new_lines::Config::resolve(cfg);
        if let Some(updated) =
            new_lines::fix(&content, rule_cfg, new_lines::platform_newline())
        {
            content = updated;
        }
    }

    if rule_enabled(SAFE_FIX_RULES[1], cfg, path, base_dir) {
        let rule_cfg = comments::Config::resolve(cfg);
        if let Some(updated) = comments::fix(&content, &rule_cfg) {
            content = updated;
        }
    }

    if rule_enabled(SAFE_FIX_RULES[2], cfg, path, base_dir) {
        let newline = target_newline(&content, cfg);
        if let Some(updated) = new_line_at_end_of_file::fix(&content, newline.as_str())
        {
            content = updated;
        }
    }

    content
}

fn rule_enabled(
    rule: RuleFix,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> bool {
    match rule.safety {
        FixSafety::Safe => {
            cfg.rule_level(rule.rule).is_some()
                && !cfg.is_rule_ignored(rule.rule, path, base_dir)
                && cfg.fix().allows_rule(rule.rule)
        }
    }
}

fn target_newline(content: &str, cfg: &YamlLintConfig) -> String {
    if cfg.rule_level(new_lines::ID).is_some() {
        return new_lines::expected_newline(
            new_lines::Config::resolve(cfg),
            new_lines::platform_newline(),
        )
        .into_owned();
    }

    first_newline(content).unwrap_or("\n").to_string()
}

fn first_newline(content: &str) -> Option<&'static str> {
    let bytes = content.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' if bytes.get(idx + 1) == Some(&b'\n') => return Some("\r\n"),
            b'\n' => return Some("\n"),
            _ => idx += 1,
        }
    }
    None
}

use std::path::{Path, PathBuf};

use crate::config::YamlLintConfig;
use crate::decoder;
use crate::rules::{
    braces, brackets, commas, comments, comments_indentation, new_line_at_end_of_file,
    new_lines, quoted_strings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixSafety {
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuleFix {
    rule: &'static str,
    safety: FixSafety,
}

const NEW_LINES_FIX: RuleFix = RuleFix {
    rule: new_lines::ID,
    safety: FixSafety::Safe,
};
const COMMENTS_FIX: RuleFix = RuleFix {
    rule: comments::ID,
    safety: FixSafety::Safe,
};
const COMMENTS_INDENTATION_FIX: RuleFix = RuleFix {
    rule: comments_indentation::ID,
    safety: FixSafety::Safe,
};
const COMMAS_FIX: RuleFix = RuleFix {
    rule: commas::ID,
    safety: FixSafety::Safe,
};
const BRACES_FIX: RuleFix = RuleFix {
    rule: braces::ID,
    safety: FixSafety::Safe,
};
const BRACKETS_FIX: RuleFix = RuleFix {
    rule: brackets::ID,
    safety: FixSafety::Safe,
};
const FINAL_NEWLINE_FIX: RuleFix = RuleFix {
    rule: new_line_at_end_of_file::ID,
    safety: FixSafety::Safe,
};
const QUOTED_STRINGS_FIX: RuleFix = RuleFix {
    rule: quoted_strings::ID,
    safety: FixSafety::Safe,
};

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
    let decoded = decoder::read_file_lossless(path)?;
    let fixed = apply_safe_fixes(decoded.content(), cfg, path, base_dir);
    if fixed == decoded.content() {
        return Ok(false);
    }

    decoded.write(path, &fixed)?;
    Ok(true)
}

/// Apply all currently supported safe fixes to each discovered file in place.
///
/// # Errors
///
/// Returns an error if any file cannot be read or any fixed contents cannot be written.
pub fn apply_safe_fixes_to_files(
    files: &[(PathBuf, PathBuf, YamlLintConfig)],
) -> Result<FixStats, String> {
    let mut stats = FixStats::default();
    for (path, base_dir, cfg) in files {
        if apply_safe_fixes_in_place(path, cfg, base_dir)? {
            stats.changed_files += 1;
        }
    }
    Ok(stats)
}

#[must_use]
pub fn apply_safe_fixes(
    input: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> String {
    let mut content = input.to_string();

    content = apply_rule_fix(content, NEW_LINES_FIX, cfg, path, base_dir, |buffer| {
        new_lines::fix(
            buffer,
            new_lines::Config::resolve(cfg),
            new_lines::platform_newline(),
        )
    });
    content = apply_rule_fix(content, COMMENTS_FIX, cfg, path, base_dir, |buffer| {
        comments::fix(buffer, &comments::Config::resolve(cfg))
    });
    content = apply_rule_fix(
        content,
        COMMENTS_INDENTATION_FIX,
        cfg,
        path,
        base_dir,
        |buffer| {
            comments_indentation::fix(
                buffer,
                &comments_indentation::Config::resolve(cfg),
            )
        },
    );
    content = apply_rule_fix(content, COMMAS_FIX, cfg, path, base_dir, |buffer| {
        commas::fix(buffer, &commas::Config::resolve(cfg))
    });
    content = apply_rule_fix(content, BRACES_FIX, cfg, path, base_dir, |buffer| {
        braces::fix(buffer, &braces::Config::resolve(cfg))
    });
    content = apply_rule_fix(content, BRACKETS_FIX, cfg, path, base_dir, |buffer| {
        brackets::fix(buffer, &brackets::Config::resolve(cfg))
    });
    content =
        apply_rule_fix(content, FINAL_NEWLINE_FIX, cfg, path, base_dir, |buffer| {
            let newline = target_newline(buffer, cfg, path, base_dir);
            new_line_at_end_of_file::fix(buffer, newline.as_str())
        });
    content =
        apply_rule_fix(content, QUOTED_STRINGS_FIX, cfg, path, base_dir, |buffer| {
            quoted_strings::fix(buffer, &quoted_strings::Config::resolve(cfg))
        });

    content
}

fn apply_rule_fix(
    content: String,
    rule: RuleFix,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    fix: impl FnOnce(&str) -> Option<String>,
) -> String {
    if !rule_enabled(rule, cfg, path, base_dir) {
        return content;
    }

    fix(&content).unwrap_or(content)
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

fn target_newline(
    content: &str,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
) -> String {
    if cfg.rule_level(new_lines::ID).is_some()
        && !cfg.is_rule_ignored(new_lines::ID, path, base_dir)
    {
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

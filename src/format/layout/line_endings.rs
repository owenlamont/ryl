use crate::rules::new_lines::{Config, LineKind, platform_newline};

pub(super) enum Candidate {
    Configured(Config),
    Retained(&'static str),
}

pub(super) struct Target {
    normalized: Option<&'static str>,
    folding: &'static str,
}

impl Candidate {
    pub(super) fn resolve(self) -> Target {
        match self {
            Self::Configured(cfg) => {
                let newline = match cfg.kind {
                    LineKind::Unix => "\n",
                    LineKind::Dos => "\r\n",
                    LineKind::Platform => platform_newline(),
                };
                Target {
                    normalized: Some(newline),
                    folding: newline,
                }
            }
            Self::Retained(newline) => Target {
                normalized: None,
                folding: newline,
            },
        }
    }
}

impl Target {
    pub(super) const fn normalizes(&self) -> bool {
        self.normalized.is_some()
    }
    pub(super) fn line<'a>(&self, retained: &'a str, allowed: bool) -> &'a str {
        self.normalized
            .filter(|_| allowed && !retained.is_empty())
            .unwrap_or(retained)
    }

    pub(super) const fn folding(&self) -> &'static str {
        self.folding
    }

    pub(super) fn boundary(&self, text: String, allowed: bool) -> String {
        match self.normalized.filter(|_| allowed) {
            Some(newline) => text
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\n', newline),
            None => text,
        }
    }
}

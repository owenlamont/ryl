//! The `JSONPath` (RFC 9535) subset that selects mappings: `$`, `.name`, `['name']`,
//! `[*]` and `.*`.

#[derive(Debug, Clone)]
pub(crate) enum Selector {
    Name(String),
    Wild,
}

/// One step from a document root: a plain key's value, a sequence item, or a node no
/// path can name (a key, or the value of a non-scalar key).
#[derive(Debug, Clone)]
pub(crate) enum Step {
    Key(String),
    Item,
    Opaque,
}

pub(crate) fn matches(selectors: &[Selector], steps: &[Step]) -> bool {
    selectors.len() == steps.len()
        && selectors.iter().zip(steps).all(|pair| match pair {
            (Selector::Wild, step) => !matches!(step, Step::Opaque),
            (Selector::Name(name), step) => {
                matches!(step, Step::Key(key) if key == name)
            }
        })
}

/// # Errors
/// Returns why `path` is not in the supported subset.
pub(crate) fn parse(path: &str) -> Result<Vec<Selector>, String> {
    let mut rest = path.strip_prefix('$').ok_or("it must start with `$`")?;
    let mut selectors = Vec::new();
    while !rest.is_empty() {
        let (next, tail) = selector(rest)?;
        selectors.push(next);
        rest = tail;
    }
    Ok(selectors)
}

fn selector(rest: &str) -> Result<(Selector, &str), String> {
    if rest.starts_with("..") {
        return Err("descendant segments (`..`) are not supported".to_owned());
    }
    if let Some(tail) = rest.strip_prefix(".*").or(rest.strip_prefix("[*]")) {
        return Ok((Selector::Wild, tail));
    }
    if let Some(tail) = rest.strip_prefix('.') {
        return dotted_name(tail).ok_or_else(|| {
            format!("`{rest}` needs a bracketed name, such as ['my-key']")
        });
    }
    if rest.starts_with("['") || rest.starts_with("[\"") {
        return quoted(&rest[1..])
            .ok_or_else(|| format!("`{rest}` has a malformed name"));
    }
    if rest.starts_with('[')
        && rest[1..].starts_with(|c: char| c.is_ascii_digit() || c == '-')
    {
        return Err("array indices are not supported; use `[*]`".to_owned());
    }
    Err(format!("`{rest}` is not a supported selector"))
}

/// An RFC 9535 shorthand name: no leading digit, then letters, digits, `_` or non-ASCII.
fn dotted_name(text: &str) -> Option<(Selector, &str)> {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()))
        .unwrap_or(text.len());
    (end > 0 && !text.starts_with(|c: char| c.is_ascii_digit()))
        .then(|| (Selector::Name(text[..end].to_owned()), &text[end..]))
}

/// A quoted name and the text after its closing `]`; only the quote and `\` escape.
fn quoted(text: &str) -> Option<(Selector, &str)> {
    let quote = char::from(text.as_bytes()[0]);
    let mut name = String::new();
    let mut chars = text[1..].char_indices();
    while let Some((index, c)) = chars.next() {
        if c == quote {
            let tail = text[index + 2..].strip_prefix(']')?;
            return Some((Selector::Name(name), tail));
        }
        name.push(if c == '\\' {
            chars
                .next()
                .map(|(_, c)| c)
                .filter(|&c| c == quote || c == '\\')?
        } else {
            c
        });
    }
    None
}

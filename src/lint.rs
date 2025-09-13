use std::fs;
use std::path::Path;

struct NullSink;
impl<'i> saphyr_parser::EventReceiver<'i> for NullSink {
    fn on_event(&mut self, _ev: saphyr_parser::Event<'i>) {}
}

/// Parse a single YAML file and return an error message formatted like yamllint on failure.
///
/// # Errors
///
/// Returns `Err(String)` when the file cannot be read or when the YAML parser
/// reports a syntax error. The error string matches the CLI output format.
pub fn parse_yaml_file(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let mut parser = saphyr_parser::Parser::new_from_str(&content);
    let mut sink = NullSink;
    match parser.load(&mut sink, true) {
        Ok(()) => Ok(()),
        Err(e) => {
            let m = e.marker();
            let msg = e.info();
            Err(format!(
                "{}\n  {}:{}       error    syntax error: {} (syntax)",
                path.display(),
                m.line(),
                m.col() + 1,
                msg
            ))
        }
    }
}

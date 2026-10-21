//! Build small, reproducible corpora containing raw regressions and generator control bytes.

pub const TARGETS: &[&str] = &[
    "aliases_merges",
    "directives_tags",
    "duplicate_keys",
    "fallible_input",
    "flow_collections",
    "large_scalars",
    "options",
    "parse_any",
    "parser_stack",
    "scan_any",
];

pub const RAW_INPUTS: &[&str] = &[
    "%FUTURE option#value\t# café\n---\nvalue\n",
    "%TAG ! tag:example.org,2026:\n--- [! value, !item value]\n",
    "{\"key\":\tvalue, empty:\t}\n",
    "|+\nα\nβ\n\n--- # next\nsecond\n",
    ">-\r\nα\r\nβ\r\n---\r\nsecond\r\n",
    "[a | b, a\n >b, é|b]\n",
    "{key: [pré,\n >bad]}\n",
    "key:\t# comment\n\tvalue\n",
    "[k: &a #\n*a]\n",
    "node:\n  &a !t #\n  &b value\nalias: *a\n",
    "&a [*a, {key: [",
    "a:\n-\nb",
    "\0",
    "[\"\\0\", '\u{7f}']",
    "?",
    "{?\t[é]: value}",
    "outer:\n  key: \"one\n   two\"\n",
    "|+\n  a\n  ",
    "clé:\r\n- é\r\n? suivant\r\n",
];

// Conforming and under-indented inputs exercise strict-mode errors as well as successful traces.
const STRICT_INPUTS: &[&str] = &[
    "key: [ # opening\n# unindented comment\n value\n ]\n",
    "key: [ # opening\n# unindented comment\nvalue\n ]\n",
    "key: [first,\n\tsecond\n ]\n",
    "key: \"é\\\r\n\" # closing\r\n",
    "key: \"é\\\r\n \" # closing\r\n",
    "key: ['é\n'] # closing\n",
];

/// Build reproducible raw inputs or generator control bytes for a fuzz target.
///
/// # Panics
/// Panics if `target` is not one of [`TARGETS`].
#[must_use]
pub fn seeds(target: &str) -> Vec<Vec<u8>> {
    let mut headers = Vec::new();
    match target {
        "parse_any" | "scan_any" => {
            return RAW_INPUTS
                .iter()
                .map(|input| input.as_bytes().to_vec())
                .collect();
        }
        "aliases_merges" => headers.extend((0..4).map(|mode| vec![mode])),
        "duplicate_keys" => headers.extend((0..5).map(|mode| vec![mode])),
        "directives_tags" => headers.extend((0..6).chain(240..=255).map(|mode| vec![mode])),
        "flow_collections" => {
            for mode in (0..6).chain(252..=254) {
                headers.extend((0..4).map(|shape| vec![mode, shape]));
            }
        }
        "large_scalars" => {
            headers.extend((0..8).map(|mode| vec![mode, 0]));
            for variant in [0, 1, 2, 3, 4, 5, 255] {
                headers.push(vec![254, 0, variant]);
            }
        }
        "options" => {
            for mode in [0, 1, 2, 252, 253, 254] {
                for selector in [0, 1, 2, 3, 7, 8, 9, 10, 11] {
                    headers.push(vec![mode, 1, 6, selector, 4]);
                }
            }
        }
        "parser_stack" => {
            for backend in 0..7 {
                for tail in 0..5 {
                    headers.push(vec![backend, tail, 2, backend % 3, backend % 2]);
                }
            }
        }
        "fallible_input" => {
            for kind in 0..3 {
                for position in [0, 1, 7, 8, 15, 255] {
                    headers.push(vec![kind, 0, position]);
                }
            }
        }
        _ => panic!("unknown fuzz target: {target}"),
    }
    for header in &mut headers {
        header.extend_from_slice("é🪨 # ---\tvalue\n".as_bytes());
    }
    if target == "options" {
        for input in STRICT_INPUTS {
            for mode in 0..3 {
                for flags in [0, 1, 0x80, 0x81] {
                    // Keep resource limits high enough to reach indentation checks.
                    let mut seed = vec![mode, flags, 6, 8, 4];
                    seed.extend_from_slice(input.as_bytes());
                    headers.push(seed);
                }
            }
        }
    }
    headers
}

#[cfg(not(test))]
fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let target = args
        .next()
        .expect("usage: seed_corpus TARGET OUTPUT_DIRECTORY");
    assert!(
        TARGETS.contains(&target.as_str()),
        "unknown fuzz target: {target}"
    );
    let directory = args.next().expect("missing output directory");
    assert!(args.next().is_none(), "unexpected extra arguments");
    std::fs::create_dir_all(&directory)?;
    for (index, input) in seeds(&target).iter().enumerate() {
        use std::io::Write;
        let path = std::path::Path::new(&directory).join(format!("changelog-seed-{index:03}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => file.write_all(input)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

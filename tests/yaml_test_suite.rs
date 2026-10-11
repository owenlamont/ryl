use std::{fs, path::Path, process::Command};

use granit_parser::{Event, Parser, ScalarStyle};

const CONFIG: &str =
    "[format]\nquote-style = 'preserve'\n[lint.rules.document-start]\n";

const SUITE_COMMIT: &str = "6ad3d2c62885d82fc349026c136ef560838fdf3d";

fn normalize(expected: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    expected
        .lines()
        .map(|line| {
            let mut line = line.to_owned();
            line = match line.as_str() {
                "+DOC ---" => "+DOC".to_owned(),
                "-DOC ..." => "-DOC".to_owned(),
                value if value.starts_with("+SEQ []") => {
                    value.replacen("+SEQ []", "+SEQ", 1)
                }
                value if value.starts_with("+MAP {}") => {
                    value.replacen("+MAP {}", "+MAP", 1)
                }
                _ => line,
            };

            if let Some(start) = line.find('&')
                && (line.starts_with("+MAP &")
                    || line.starts_with("+SEQ &")
                    || line.starts_with("=VAL &"))
            {
                let end = line[start..].find(' ').map_or(line.len(), |n| start + n);
                anchors.push(line[start + 1..end].to_owned());
                line.replace_range(start..end, &format!("&{}", anchors.len()));
            }
            if let Some(name) = line.strip_prefix("=ALI *") {
                let index = anchors.iter().rposition(|anchor| anchor == name).unwrap();
                line = format!("=ALI *{}", index + 1);
            }
            if line.starts_with("=VAL ") && !line.contains('<') && line.ends_with(" :")
            {
                line.push('~');
            }
            line
        })
        .collect()
}

fn events(input: &str) -> Result<Vec<String>, String> {
    Parser::new_from_str(input)
        .filter_map(|item| {
            let (event, _) = match item {
                Ok(event) => event,
                Err(error) => return Some(Err(error.to_string())),
            };
            let anchor = event
                .anchor_id()
                .filter(|id| *id > 0)
                .map_or_else(String::new, |id| format!(" &{id}"));
            let tag = event.tag().map_or_else(String::new, |tag| {
                format!(" <{}{}>", tag.handle(), tag.suffix())
            });
            let line = match event {
                Event::StreamStart => "+STR".to_owned(),
                Event::StreamEnd => "-STR".to_owned(),
                Event::DocumentStart(..) => "+DOC".to_owned(),
                Event::DocumentEnd => "-DOC".to_owned(),
                Event::MappingStart(..) => format!("+MAP{anchor}{tag}"),
                Event::MappingEnd => "-MAP".to_owned(),
                Event::SequenceStart(..) => format!("+SEQ{anchor}{tag}"),
                Event::SequenceEnd => "-SEQ".to_owned(),
                Event::Alias(id) => format!("=ALI *{id}"),
                Event::Scalar(text, style, ..) => {
                    let style = match style {
                        ScalarStyle::Plain => ':',
                        ScalarStyle::SingleQuoted => '\'',
                        ScalarStyle::DoubleQuoted => '"',
                        ScalarStyle::Literal => '|',
                        ScalarStyle::Folded => '>',
                    };
                    let text = text
                        .replace('\\', "\\\\")
                        .replace('\n', "\\n")
                        .replace('\r', "\\r")
                        .replace('\t', "\\t")
                        .replace('\x08', "\\b");
                    format!("=VAL{anchor}{tag} {style}{text}")
                }
                _ => return None,
            };
            Some(Ok(line))
        })
        .collect()
}

fn cli(config: &Path, args: &[&str], file: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(args)
        .arg("-c")
        .arg(config)
        .arg(file)
        .env_remove("GITHUB_ACTIONS")
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires a separately fetched, pinned yaml-test-suite data checkout"]
fn yaml_test_suite_gate() {
    let suite = std::env::var_os("YAML_TEST_SUITE").expect("set YAML_TEST_SUITE");
    let suite = Path::new(&suite);
    let revision = Command::new("git")
        .arg("-C")
        .arg(suite)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(revision.status.success());
    assert_eq!(
        String::from_utf8(revision.stdout).unwrap().trim(),
        SUITE_COMMIT
    );
    let clean = Command::new("git")
        .arg("-C")
        .arg(suite)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        clean.status.success() && clean.stdout.is_empty(),
        "suite must be clean"
    );
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("gate.toml");
    fs::write(&config, CONFIG).unwrap();
    let cfg = ryl::config::YamlLintConfig::from_toml_str(CONFIG).unwrap();
    let file = temp.path().join("case.yaml");
    let mut cases = Vec::new();
    for entry in fs::read_dir(suite).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if name.len() != 4
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            continue;
        }
        if path.join("in.yaml").is_file() {
            cases.push(path);
        } else if path.is_dir() {
            cases.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.join("in.yaml").is_file()),
            );
        }
    }
    cases.sort();
    assert_eq!(cases.len(), 402, "pinned suite inventory");
    let (mut passed, mut failed, mut bails) = (0, 0, 0);
    for case in cases {
        let id = case.strip_prefix(suite).unwrap().to_string_lossy();
        let input = fs::read_to_string(case.join("in.yaml")).unwrap();
        fs::write(&file, &input).unwrap();
        let formatted = cli(&config, &["format"], &file);
        let output = fs::read_to_string(&file).unwrap();
        let stderr = String::from_utf8_lossy(&formatted.stderr);
        let failure = if case.join("error").exists() {
            let checked = cli(&config, &["check"], &file);
            if !formatted.status.success()
                || output != input
                || !stderr.contains("skipped by ryl format")
            {
                Some("format did not refuse invalid YAML".to_owned())
            } else if checked.status.code() != Some(1)
                || !(String::from_utf8_lossy(&checked.stdout).contains("syntax")
                    || String::from_utf8_lossy(&checked.stderr).contains("syntax"))
            {
                Some("check did not report a syntax error".to_owned())
            } else {
                None
            }
        } else {
            let target = ryl::rules::indentation::Config::new(
                ryl::rules::indentation::SpacesSetting::Fixed(usize::from(
                    ryl::format::file_indent_width(&cfg, &input),
                )),
                ryl::rules::indentation::IndentSequencesSetting::True,
                false,
            );
            let refused = ryl::rules::indentation::reindent(&input, &target).refused;
            let other_refusals = ryl::format::refusals(&input, &cfg);
            let expected =
                normalize(&fs::read_to_string(case.join("test.event")).unwrap());
            match (events(&input), events(&output)) {
                (Err(error), _) => Some(format!("parser rejects valid input: {error}")),
                (_, Err(error)) => {
                    Some(format!("formatted output is invalid: {error}"))
                }
                (Ok(before), Ok(after)) if before != expected || after != expected => {
                    let (phase, actual) = if before != expected {
                        ("input", before)
                    } else {
                        ("output", after)
                    };
                    let index = actual
                        .iter()
                        .zip(&expected)
                        .position(|(a, e)| a != e)
                        .unwrap_or(actual.len().min(expected.len()));
                    Some(format!(
                        "{phase} events differ at {index}: expected {:?}, got {:?}",
                        expected.get(index),
                        actual.get(index)
                    ))
                }
                _ if !formatted.status.success() => {
                    Some("format command failed".to_owned())
                }
                _ => {
                    let again = cli(&config, &["format"], &file);
                    if !again.status.success()
                        || fs::read_to_string(&file).unwrap() != output
                    {
                        Some("format is not idempotent".to_owned())
                    } else if !refused.is_empty() || !other_refusals.is_empty() {
                        bails += 1;
                        println!(
                            "BAIL {id}: reindent={refused:?}; collections={other_refusals:?}"
                        );
                        continue;
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(reason) = failure {
            failed += 1;
            println!("FAIL {id}: {}", reason.replace('\n', " "));
        } else {
            passed += 1;
        }
    }
    println!("RESULT passed={passed} failed={failed} bail-outs={bails}");
    assert_eq!(failed, 0, "yaml-test-suite gate failures");
}

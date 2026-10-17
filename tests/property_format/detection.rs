use ryl::config::YamlLintConfig;
use ryl::rules::indentation::{self, IndentSequencesSetting, SpacesSetting};
use similar::{DiffTag, TextDiff};

pub fn detection_target(cfg: &YamlLintConfig, width: u8) -> indentation::Config {
    indentation::Config::new(
        SpacesSetting::Fixed(usize::from(width)),
        if cfg.format().targets().indent_sequences {
            IndentSequencesSetting::True
        } else {
            IndentSequencesSetting::False
        },
        false,
    )
    .with_dash_on_own_line(cfg.format().targets().dash_on_own_line)
}

pub fn check_settled_candidates(cfg: &YamlLintConfig, input: &str) {
    for width in 2..=8 {
        let target = detection_target(cfg, width);
        let first = indentation::reindent(input, &target);
        let actual = ryl::format::settled(first.text.clone(), input, cfg, width);
        let mut expected = first;
        let mut previous = input.to_owned();
        for _ in 0..ryl::fix::FIX_PIPELINE_MAX_PASSES {
            if expected.text == previous {
                break;
            }
            let next = indentation::reindent(&expected.text, &target);
            previous = std::mem::replace(&mut expected, next).text;
        }
        assert_eq!(actual, expected.text, "width {width}: {input:?}");
        assert_eq!(
            indentation::reindent(&actual, &target),
            expected,
            "settled refusal at width {width}: {input:?}",
        );
    }
}

pub fn original_indent_width(cfg: &YamlLintConfig, input: &str) -> u8 {
    if let Some(width) = cfg.indent_width() {
        return width.get();
    }
    if granit_parser::Parser::new_from_str(input).all(|event| event.is_ok())
        && let Some(width) = observed_indent_width(input)
    {
        let candidate = indentation::reindent(input, &detection_target(cfg, width));
        if candidate.refused.is_empty() && candidate.text == input {
            return width;
        }
    }
    if indentation::reindent(input, &detection_target(cfg, 2)).text == input {
        return 2;
    }
    let consistent = indentation::Config::new(
        SpacesSetting::Consistent,
        if cfg.format().targets().indent_sequences {
            IndentSequencesSetting::True
        } else {
            IndentSequencesSetting::False
        },
        false,
    );
    if indentation::has_fixed_explicit_indent(input, &consistent) {
        return 2;
    }
    let before: Vec<&str> = input.split_inclusive('\n').collect();
    let scores: Vec<(u8, usize)> = (2..=8)
        .map(|width| {
            let mut text =
                indentation::reindent(input, &detection_target(cfg, width)).text;
            let mut previous = input.to_string();
            for _ in 0..ryl::fix::FIX_PIPELINE_MAX_PASSES {
                if text == previous {
                    break;
                }
                let next =
                    indentation::reindent(&text, &detection_target(cfg, width)).text;
                previous = std::mem::replace(&mut text, next);
            }
            let after: Vec<&str> = text.split_inclusive('\n').collect();
            let changed = changed_lines(&before, &after);
            (width, changed)
        })
        .collect();
    let fewest = scores.iter().map(|&(_, count)| count).min();
    let winners: Vec<_> = scores
        .iter()
        .filter(|&&(_, count)| Some(count) == fewest)
        .collect();
    match winners[..] {
        [&(width, _)] => width,
        _ => 2,
    }
}

pub fn changed_lines(before: &[&str], after: &[&str]) -> usize {
    TextDiff::from_slices(before, after)
        .ops()
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| op.old_range().len().max(op.new_range().len()))
        .sum()
}

fn observed_indent_width(content: &str) -> Option<u8> {
    let mut width = None;
    for line in content.lines().filter(|line| !line.is_empty()) {
        let indent = line.bytes().take_while(|&byte| byte == b' ').count();
        let (key, value) = line[indent..].split_once(':')?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || (!value.is_empty() && !value.starts_with(' '))
            || value.trim_start().starts_with(['|', '>'])
        {
            return None;
        }
        if indent == 0 {
            if !value.is_empty() {
                return None;
            }
        } else {
            let indent = u8::try_from(indent).ok()?;
            if !(2..=8).contains(&indent) || value.is_empty() {
                return None;
            }
            if width
                .replace(indent)
                .is_some_and(|previous| previous != indent)
            {
                return None;
            }
        }
    }
    width.filter(|&width| width != 2)
}

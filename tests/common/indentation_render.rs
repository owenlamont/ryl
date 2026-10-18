use ryl::rules::indentation::{
    Config, IndentSequencesSetting, SpacesSetting, reindent,
};
use serde_json::{Value, json};

pub type Request = (String, usize, usize, Option<bool>, bool);

pub fn render((input, width, sequences, dash, multi_line): &Request) -> Value {
    let mut cfg = Config::new(
        SpacesSetting::Fixed(*width),
        [
            IndentSequencesSetting::True,
            IndentSequencesSetting::False,
            IndentSequencesSetting::Whatever,
            IndentSequencesSetting::Consistent,
        ][*sequences],
        *multi_line,
    );
    if let Some(dash) = dash {
        cfg = cfg.with_dash_on_own_line(*dash);
    }
    let output = reindent(input, &cfg);
    let refused: Vec<_> = output
        .refused
        .iter()
        .map(|refusal| {
            (
                *refusal.lines.start(),
                *refusal.lines.end(),
                format!("{:?}", refusal.cause),
            )
        })
        .collect();
    let table = format!(
        "{}[format]\ndash-on-own-line = {}\nindent-sequences = {}\n",
        if *multi_line {
            format!("indent-width = {width}\n")
        } else {
            String::new()
        },
        dash.unwrap_or(false),
        *sequences != 1,
    );
    let format_cfg = ryl::config::YamlLintConfig::from_toml_str(&table).unwrap();
    let formatted = ryl::format::format_str(
        input,
        &format_cfg,
        std::path::Path::new("input.yaml"),
        &[],
    );
    let notices = format!("{:?}", ryl::format::refusals(input, &format_cfg));
    json!([
        output.text,
        refused,
        output.kept_dash_lines,
        formatted,
        notices
    ])
}

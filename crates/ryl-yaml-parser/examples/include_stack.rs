use granit_parser::{Event, Parser, ParserStack, ParserTrait, ScanError, StrInput, Tag};

type IncludeStack = ParserStack<'static, core::iter::Empty<char>, StrInput<'static>>;

const ROOT_YAML: &str = r"
root:
  before: &root_anchor from-root
  include: !include something.yaml
  after: &after_anchor after-include
";

fn resolve_include(name: &str) -> &'static str {
    match name {
        "something.yaml" => "included: &included_anchor from-include\n",
        _ => unreachable!("the example only resolves its static include"),
    }
}

fn is_include_tag(tag: &Tag) -> bool {
    tag.parts() == ("!", "include")
}

fn main() -> Result<(), ScanError> {
    let mut stack: IncludeStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str(ROOT_YAML), "root.yaml".to_string());
    stack.set_borrowed_resolver(|name| Ok(resolve_include(name)));

    let mut anchored_scalars = Vec::new();

    while let Some(next) = stack.next_event() {
        let (event, _) = next?;

        match event {
            Event::Scalar(include_name, _, _, Some(tag)) if is_include_tag(tag.as_ref()) => {
                stack.push_include(include_name.as_ref())?;
            }
            Event::Scalar(value, _, anchor_id, _) if anchor_id > 0 => {
                anchored_scalars.push((value.into_owned(), anchor_id));
            }
            _ => {}
        }
    }

    assert_eq!(
        anchored_scalars,
        vec![
            ("from-root".to_string(), 1),
            ("from-include".to_string(), 2),
            ("after-include".to_string(), 3),
        ]
    );

    for (value, anchor_id) in anchored_scalars {
        println!("{value}: anchor {anchor_id}");
    }

    Ok(())
}

use ryl::yaml_dom::YamlOwned;

pub fn parse_for_compare(content: &str) -> Option<Vec<YamlOwned>> {
    let docs = YamlOwned::load_from_str(content).ok()?;
    Some(docs.into_iter().map(canonical).collect())
}

fn canonical(node: YamlOwned) -> YamlOwned {
    match node {
        YamlOwned::Mapping(mapping) => {
            let mut entries: Vec<_> = mapping
                .into_iter()
                .map(|(key, value)| (canonical(key), canonical(value)))
                .collect();
            entries.sort_by_cached_key(|(key, _)| format!("{key:?}"));
            YamlOwned::Mapping(entries.into_iter().collect())
        }
        YamlOwned::Sequence(items) => {
            YamlOwned::Sequence(items.into_iter().map(canonical).collect())
        }
        YamlOwned::Tagged(tag, inner) => {
            YamlOwned::Tagged(tag, Box::new(canonical(*inner)))
        }
        other => other,
    }
}

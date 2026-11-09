use std::cmp::Reverse;

use jsonschema::{
    JsonType,
    error::{ValidationError, ValidationErrorKind},
};
use serde_json::Value;

use super::Association;

type Relevance = (Reverse<usize>, bool, bool);

pub(super) fn unevaluated_item_index(
    items: &[Value],
    unexpected: &[String],
) -> Option<usize> {
    // Matching from the end skips identical values in evaluated prefixes.
    unexpected.iter().rev().try_fold(items.len(), |end, value| {
        items[..end].iter().rposition(|item| {
            let serialized = item.to_string();
            serialized == *value
        })
    })
}

fn relevance(error: &ValidationError<'_>, association: &Association) -> Relevance {
    let location = error
        .absolute_keyword_location()
        .expect("schema has a base URI");
    let location = location.as_str();
    let parent = if matches!(error.kind(), ValidationErrorKind::FalseSchema) {
        location
    } else {
        location
            .split_once('#')
            .and_then(|(_, fragment)| fragment.rsplit_once('/'))
            .map_or(location, |(_, keyword)| {
                &location[..location.len() - keyword.len() - 1]
            })
    };
    let resolved = association
        .registry
        .resolver(association.root_uri.clone())
        .lookup(parent)
        .ok();
    let matches_type = resolved.is_some_and(|resolved| {
        let (schema, _, draft) = resolved.into_inner();
        let matches = |kind: &Value| {
            let instance = error.instance();
            let kind = kind
                .as_str()
                .expect("schema type is a string")
                .parse::<JsonType>()
                .expect("schema type was validated at compilation");
            if kind == JsonType::Integer {
                instance.is_i64()
                    || instance.is_u64()
                    || (draft != jsonschema::Draft::Draft4
                        && instance
                            .as_f64()
                            .is_some_and(|number| number.fract() == 0.0))
            } else {
                JsonType::from(instance.as_ref()) == kind
            }
        };
        schema.get("type").is_some_and(|kind| {
            if let Value::Array(kinds) = kind {
                kinds.iter().any(matches)
            } else {
                matches(kind)
            }
        })
    });
    (
        Reverse(
            error
                .instance_path()
                .as_str()
                .bytes()
                .filter(|byte| *byte == b'/')
                .count(),
        ),
        !matches!(error.kind().keyword(), "anyOf" | "oneOf"),
        !matches_type,
    )
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PathPart {
    Index(usize),
    Key(String),
}

fn instance_path(error: &ValidationError<'_>, mut root: &Value) -> Vec<PathPart> {
    error
        .instance_path()
        .as_str()
        .split('/')
        .skip(1)
        .map(|segment| {
            let key = segment.replace("~1", "/").replace("~0", "~");
            let part = if root.is_array() {
                PathPart::Index(key.parse().expect("array error path has an index"))
            } else {
                PathPart::Key(key.clone())
            };
            root = root
                .pointer(&format!("/{segment}"))
                .expect("error instance path exists");
            part
        })
        .collect()
}

// Python jsonschema's best_match retains ambiguous alternatives instead of guessing.
pub(super) fn best_match<'a, 'i>(
    mut error: &'a ValidationError<'i>,
    association: &Association,
    root: &Value,
) -> &'a ValidationError<'i> {
    while let ValidationErrorKind::AnyOf { context }
    | ValidationErrorKind::OneOfNotValid { context } = error.kind()
    {
        let mut branches = context
            .iter()
            .filter_map(|errors| {
                errors
                    .iter()
                    .map(|child| (relevance(child, association), child))
                    .reduce(|best, candidate| {
                        if candidate.0 > best.0
                            || (candidate.0 == best.0
                                && instance_path(candidate.1, root)
                                    < instance_path(best.1, root))
                        {
                            candidate
                        } else {
                            best
                        }
                    })
                    .map(|(key, child)| ((key, errors.len()), child))
            })
            .collect::<Vec<_>>();
        branches.sort_by_key(|(rank, _)| *rank);
        let (rank, best) = branches
            .first()
            .expect("failing alternatives have branch errors");
        if branches.get(1).is_some_and(|(next, _)| next == rank) {
            break;
        }
        error = best;
    }
    error
}

use std::collections::HashSet;

use jsonschema::Draft;
use ryl_yaml_parser::{Event, Marker, ScalarStyle, Span};
use serde_json::Value;

use super::{Association, projection};
use crate::rules::support::span_utils::marker_byte_offset;
use crate::text_version::TextVersion;

pub(crate) fn at<'a>(
    association: &'a Association,
    text: &str,
    position: (usize, usize),
) -> Option<([Option<&'a str>; 2], Span)> {
    let version = TextVersion::new(text);
    let omitted = version
        .analysis()
        .event_iter()
        .filter_map(|(event, span)| {
            (span.is_empty()
                && matches!(event, Event::Scalar(_, ScalarStyle::Plain, 0, None)))
            .then_some(span.start.index())
        })
        .collect::<HashSet<_>>();
    let mut events = version.analysis().event_iter().peekable();
    let mut budget = projection::Budget::default();
    loop {
        let projected = match projection::document(&mut events, &mut budget) {
            Ok(Some(projected)) => projected,
            Err(_) if !budget.exhausted() => continue,
            Ok(None) | Err(_) => return None,
        };
        if projected.ambiguous {
            continue;
        }
        let Some((pointer, span)) = projected
            .nodes
            .iter()
            .map(|(pointer, span)| {
                let mut span = *span;
                if span.is_empty()
                    && omitted.contains(&span.start.index())
                    && let Some(key) = projected.keys.get(pointer)
                {
                    let start = span.start;
                    let offset = marker_byte_offset(start).get();
                    span = if text.as_bytes().get(offset) == Some(&b':') {
                        Span::empty(
                            Marker::new(
                                start.index() + 1,
                                start.line(),
                                start.col() + 1,
                            )
                            .with_byte_offset(Some(offset + 1)),
                        )
                    } else {
                        Span::empty(key.end)
                    };
                }
                (pointer, span)
            })
            .chain(
                projected
                    .keys
                    .iter()
                    .map(|(pointer, span)| (pointer, *span)),
            )
            .filter(|(_, span)| {
                (span.start.line(), span.start.col()) <= position
                    && (position < (span.end.line(), span.end.col())
                        || position == (span.start.line(), span.start.col()))
            })
            .min_by_key(|(_, span)| span.end.index() - span.start.index())
        else {
            continue;
        };
        return lookup(association, pointer, &projected.value)
            .map(|annotations| (annotations, span));
    }
}

fn lookup<'a>(
    association: &'a Association,
    pointer: &str,
    instance: &Value,
) -> Option<[Option<&'a str>; 2]> {
    let root = association
        .registry
        .resolver(association.root_uri.clone())
        .lookup("#")
        .expect("prepared registry contains the root");
    let (mut value, mut resolver, mut draft) = root.into_inner();
    resolver = resolver
        .in_subresource(draft.create_resource_ref(value))
        .expect("prepared root has a valid scope");
    let tokens = pointer
        .split('/')
        .skip(1)
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    let mut remaining = tokens.as_slice();
    let mut instance = instance;
    let mut seen = HashSet::new();
    let mut annotations = [None; 2];
    let mut steps = 0;
    while steps < 128 && seen.insert((std::ptr::from_ref(value), remaining.len())) {
        steps += 1;
        let old_ref = matches!(draft, Draft::Draft4 | Draft::Draft6 | Draft::Draft7)
            && value.get("$ref").is_some();
        if !old_ref && remaining.is_empty() {
            for (slot, keyword) in annotations.iter_mut().zip(["title", "description"])
            {
                let Some(text) = value.get(keyword).and_then(Value::as_str) else {
                    continue;
                };
                if slot.is_some_and(|previous| previous != text) {
                    return None;
                }
                *slot = Some(text);
            }
        }
        if ["$dynamicRef", "$recursiveRef"]
            .iter()
            .any(|key| draft.is_known_keyword(key) && value.get(key).is_some())
        {
            return None;
        }
        if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
            if !old_ref
                && !remaining.is_empty()
                && (value.get("properties").is_some() || value.get("items").is_some())
            {
                return None;
            }
            let (resource, fragment) =
                reference.rsplit_once('#').unwrap_or((reference, ""));
            let base = association
                .registry
                .resolve_uri(&resolver.base_uri().borrow(), resource)
                .expect("prepared reference has a valid URI");
            let target = association
                .registry
                .resolver(base.as_ref().clone())
                .lookup(&format!("#{fragment}"))
                .expect("prepared registry contains static references");
            (value, resolver, draft) = target.into_inner();
            continue;
        }
        let Some((token, tail)) = remaining.split_first() else {
            let annotated = annotations.iter().any(Option::is_some);
            return annotated.then_some(annotations);
        };
        let child = if instance.is_object() {
            value.get("properties")?.get(token)?
        } else {
            if draft.is_known_keyword("prefixItems")
                && value.get("prefixItems").is_some()
            {
                return None;
            }
            value
                .get("items")
                .filter(|items| items.is_object() || items.is_boolean())?
        };
        instance = if let Some(object) = instance.as_object() {
            object
                .get(token)
                .expect("projection pointer identifies an object member")
        } else {
            instance
                .get(
                    token
                        .parse::<usize>()
                        .expect("projection array pointer is an index"),
                )
                .expect("projection pointer identifies an array item")
        };
        draft = draft.detect(child);
        resolver = resolver
            .in_subresource(draft.create_resource_ref(child))
            .expect("prepared subschema has a valid scope");
        value = child;
        remaining = tail;
    }
    None
}

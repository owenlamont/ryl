use std::collections::{HashMap, VecDeque};

use jsonschema::{Draft, Registry, Uri};
use serde_json::Value;

const ARRAY_DESCENT: u8 = 1;
const OBJECT_DESCENT: u8 = 2;

const MAX_WEIGHT: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Storage,
    Same,
    Alternative,
    Property,
    Object,
    Array,
}

struct Edge {
    target: usize,
    kind: Kind,
}

#[derive(Default)]
struct Graph {
    edges: Vec<Vec<Edge>>,
    locations: Vec<String>,
}

#[derive(Default)]
struct Dynamic {
    recursive: Vec<usize>,
    anchors: HashMap<String, Vec<usize>>,
    references: Vec<(usize, &'static str, String, *const Value)>,
}

impl Dynamic {
    fn reference(
        &mut self,
        source: usize,
        keyword: &'static str,
        fragment: &str,
        child: &Value,
    ) -> Kind {
        let anchor = child.get("$dynamicAnchor").and_then(Value::as_str);
        if keyword != "$ref"
            || (anchor.is_some() && !fragment.is_empty() && !fragment.starts_with('/'))
        {
            self.references.push((
                source,
                keyword,
                anchor.unwrap_or("").to_owned(),
                std::ptr::from_ref(child),
            ));
        }
        if keyword == "$ref" {
            Kind::Same
        } else {
            Kind::Alternative
        }
    }

    fn candidates(&self, keyword: &str, anchor: &str) -> &[usize] {
        if keyword == "$recursiveRef" {
            &self.recursive
        } else {
            self.anchors.get(anchor).map_or(&[], Vec::as_slice)
        }
    }

    fn record(&mut self, value: &Value, draft: Draft, source: usize) {
        if value.get("$recursiveAnchor") == Some(&Value::Bool(true)) {
            self.recursive.push(source);
        }
        if draft.is_known_keyword("$dynamicAnchor")
            && let Some(anchor) = value.get("$dynamicAnchor").and_then(Value::as_str)
        {
            self.anchors
                .entry(anchor.to_owned())
                .or_default()
                .push(source);
        }
    }
}

fn children(value: &Value, draft: Draft) -> Vec<(String, &Value, Kind)> {
    let mut children = Vec::new();
    let Some(object) = value.as_object() else {
        return children;
    };
    if matches!(draft, Draft::Draft4 | Draft::Draft6 | Draft::Draft7)
        && object.contains_key("$ref")
    {
        return children;
    }
    for (key, value) in object {
        let kind = match key.as_str() {
            "$defs" | "definitions" => Kind::Storage,
            "properties" => Kind::Property,
            "patternProperties"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "propertyNames" => Kind::Object,
            "items" | "additionalItems" | "prefixItems" | "contains"
            | "unevaluatedItems" => Kind::Array,
            _ => Kind::Same,
        };
        let mut push = |suffix: String, child| {
            children.push((suffix, child, kind));
        };
        match key.as_str() {
            "$defs" | "definitions" | "properties" | "patternProperties"
            | "dependentSchemas" | "dependencies" => {
                for (name, child) in value
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, child)| child.is_object() || child.is_boolean())
                {
                    push(
                        format!(
                            "/{key}/{}",
                            name.replace('~', "~0").replace('/', "~1")
                        ),
                        child,
                    );
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" | "items"
                if value.is_array() =>
            {
                for (index, child) in
                    value.as_array().expect("array keyword").iter().enumerate()
                {
                    push(format!("/{key}/{index}"), child);
                }
            }
            "items"
            | "additionalItems"
            | "additionalProperties"
            | "unevaluatedItems"
            | "unevaluatedProperties"
            | "contains"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contentSchema"
                if value.is_object() || value.is_boolean() =>
            {
                push(format!("/{key}"), value);
            }
            _ => {}
        }
    }
    let active = draft
        .subresources_of(value)
        .map(std::ptr::from_ref)
        .collect::<std::collections::HashSet<_>>();
    children.retain(|(_, child, _)| active.contains(&std::ptr::from_ref(*child)));
    children
}

#[derive(Debug, thiserror::Error)]
enum BuildError {
    #[error("cannot resolve schema during complexity screening: {0}")]
    Resolution(#[from] jsonschema::ReferencingError),
    #[error("{0}")]
    Complexity(String),
}

impl Graph {
    fn build(registry: &Registry<'_>, uri: Uri<String>) -> Result<Self, BuildError> {
        let root = registry.resolver(uri).lookup("#").and_then(|root| {
            let (value, resolver, draft) = root.into_inner();
            resolver
                .in_subresource(draft.create_resource_ref(value))
                .map(|resolver| {
                    (
                        (value, resolver, draft, "#".to_owned()),
                        None::<(usize, Kind)>,
                        false,
                    )
                })
        });
        let mut graph = Self::default();
        let mut indices = HashMap::new();
        let mut pending = vec![root];
        let mut dynamic = Dynamic::default();
        while let Some(target) = pending.pop() {
            let ((value, resolver, draft, location), origin, referenced) = target?;
            if referenced {
                pending.push(resolver.lookup("#").map(|root| {
                    let (value, resolver, draft) = root.into_inner();
                    let location = resolver.base_uri().to_string();
                    ((value, resolver, draft, location), None, false)
                }));
            }
            let next = indices.len();
            let source = *indices.entry(std::ptr::from_ref(value)).or_insert(next);
            if source == next {
                graph.edges.push(Vec::new());
                graph.locations.push(location.clone());
            }
            if let Some((parent, kind)) = origin {
                graph.edges[parent].push(Edge {
                    target: source,
                    kind,
                });
            }
            if source != next {
                continue;
            }
            dynamic.record(value, draft, source);
            for (suffix, child, kind) in children(value, draft) {
                let draft = draft.detect(child);
                let child_location = format!("{location}{suffix}");
                pending.push(
                    resolver
                        .in_subresource(draft.create_resource_ref(child))
                        .map(|scope| {
                            (
                                (child, scope, draft, child_location),
                                Some((source, kind)),
                                false,
                            )
                        }),
                );
            }
            for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
                if !draft.is_known_keyword(keyword)
                    || (keyword == "$recursiveRef" && draft != Draft::Draft201909)
                {
                    continue;
                }
                let Some(reference) = value.get(keyword).and_then(Value::as_str) else {
                    continue;
                };
                let (resource, fragment) =
                    reference.rsplit_once('#').unwrap_or((reference, ""));
                let target = registry
                    .resolve_uri(&resolver.base_uri().borrow(), resource)
                    .and_then(|base| {
                        registry
                            .resolver(base.as_ref().clone())
                            .lookup(&format!("#{fragment}"))
                    });
                pending.push(target.map(|resolved| {
                    let (child, scope, draft) = resolved.into_inner();
                    let kind = dynamic.reference(source, keyword, fragment, child);
                    (
                        (child, scope, draft, reference.to_owned()),
                        Some((source, kind)),
                        true,
                    )
                }));
            }
        }
        graph
            .connect_dynamic(&dynamic, &indices)
            .map_err(BuildError::Complexity)?;
        Ok(graph)
    }

    fn connect_dynamic(
        &mut self,
        dynamic: &Dynamic,
        indices: &HashMap<*const Value, usize>,
    ) -> Result<(), String> {
        let edges = dynamic.references.iter().fold(
            0_usize,
            |total, (_, keyword, anchor, _)| {
                total.saturating_add(dynamic.candidates(keyword, anchor).len())
            },
        );
        if edges > usize::try_from(MAX_WEIGHT).expect("weight limit fits usize") {
            return Err(format!(
                "schema too complex: dynamic reference edges exceed {MAX_WEIGHT}"
            ));
        }
        for (source, keyword, anchor, static_target) in &dynamic.references {
            let kind = if *keyword == "$ref" {
                Kind::Same
            } else {
                Kind::Alternative
            };
            for &target in dynamic.candidates(keyword, anchor) {
                if target != indices[static_target] {
                    self.edges[*source].push(Edge { target, kind });
                }
            }
        }
        Ok(())
    }

    fn weight(&self) -> u64 {
        let mut weights = vec![0_u64; self.edges.len()];
        let mut active = vec![false; self.edges.len()];
        let mut stack = vec![(0, 0, 1_u64)];
        active[0] = true;
        while let Some((node, next, total)) = stack.last_mut() {
            if let Some(edge) = self.edges[*node].get(*next) {
                if active[edge.target] || weights[edge.target] != 0 {
                    *total = total.saturating_add(weights[edge.target].max(1));
                    *next += 1;
                } else {
                    active[edge.target] = true;
                    stack.push((edge.target, 0, 1));
                }
            } else {
                weights[*node] = *total;
                active[*node] = false;
                stack.pop();
            }
        }
        weights[0]
    }

    fn components(&self) -> Vec<usize> {
        let mut seen = vec![false; self.edges.len()];
        let mut order = Vec::new();
        let mut reverse = vec![Vec::new(); self.edges.len()];
        for (source, edges) in self.edges.iter().enumerate() {
            for edge in edges.iter().filter(|edge| edge.kind != Kind::Storage) {
                reverse[edge.target].push(source);
            }
            let mut stack = vec![(source, false)];
            while let Some((node, finished)) = stack.pop() {
                if finished {
                    order.push(node);
                } else if !seen[node] {
                    seen[node] = true;
                    stack.push((node, true));
                    stack.extend(
                        self.edges[node]
                            .iter()
                            .rev()
                            .filter(|e| e.kind != Kind::Storage)
                            .map(|e| (e.target, false)),
                    );
                }
            }
        }
        let mut components = vec![usize::MAX; self.edges.len()];
        for node in order.into_iter().rev() {
            let mut stack = vec![node];
            while let Some(member) = stack.pop() {
                if components[member] == usize::MAX {
                    components[member] = node;
                    stack.extend(&reverse[member]);
                }
            }
        }
        components
    }

    fn first_descent(&self, edge: &Edge, components: &[usize]) -> u8 {
        let mut kinds = 0;
        let mut pending = vec![edge];
        let mut seen = std::collections::HashSet::new();
        while let Some(edge) = pending.pop() {
            match edge.kind {
                Kind::Array => kinds |= ARRAY_DESCENT,
                Kind::Object | Kind::Property => kinds |= OBJECT_DESCENT,
                _ if seen.insert(edge.target) => {
                    let children = self.internal(edge.target, components);
                    pending.extend(children);
                }
                _ => {}
            }
        }
        kinds
    }

    fn internal(&self, node: usize, components: &[usize]) -> Vec<&Edge> {
        self.edges[node]
            .iter()
            .filter(|edge| {
                edge.kind != Kind::Storage
                    && components[edge.target] == components[node]
            })
            .collect()
    }

    fn overlaps(&self, node: usize, components: &[usize]) -> bool {
        let mut properties = false;
        let edges = self.internal(node, components);
        // A dynamic reference selects one target, so its possible descents form a union.
        let mut kinds = edges
            .iter()
            .filter(|edge| edge.kind == Kind::Alternative)
            .fold(0, |kinds, edge| {
                kinds | self.first_descent(edge, components)
            });
        for edge in edges
            .into_iter()
            .filter(|edge| edge.kind != Kind::Alternative)
        {
            if edge.kind == Kind::Property {
                if kinds & OBJECT_DESCENT != 0 {
                    return true;
                }
                properties = true;
                continue;
            }
            let descent = self.first_descent(edge, components);
            if descent & kinds != 0 || (descent & OBJECT_DESCENT != 0 && properties) {
                return true;
            }
            kinds |= descent;
        }
        false
    }

    fn recursion(&self) -> Result<(), String> {
        let components = self.components();
        let mut incoming = vec![0; self.edges.len()];
        for edges in &self.edges {
            for edge in edges
                .iter()
                .filter(|e| matches!(e.kind, Kind::Same | Kind::Alternative))
            {
                incoming[edge.target] += 1;
            }
        }
        let mut ready = incoming
            .iter()
            .enumerate()
            .filter_map(|(n, &count)| (count == 0).then_some(n))
            .collect::<VecDeque<_>>();
        while let Some(node) = ready.pop_front() {
            for edge in self.edges[node]
                .iter()
                .filter(|e| matches!(e.kind, Kind::Same | Kind::Alternative))
            {
                incoming[edge.target] -= 1;
                if incoming[edge.target] == 0 {
                    ready.push_back(edge.target);
                }
            }
        }
        if let Some(node) = incoming.iter().position(|&count| count != 0) {
            return Err(format!("non-descending cycle at {}", self.locations[node]));
        }
        for node in 0..self.edges.len() {
            if self.overlaps(node, &components) {
                return Err(format!(
                    "overlapping recursive branches at {}",
                    self.locations[node]
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn check(registry: &Registry<'_>, uri: Uri<String>) -> Result<(), String> {
    let graph = Graph::build(registry, uri).map_err(|e| e.to_string())?;
    let weight = graph.weight();
    if weight > MAX_WEIGHT {
        return Err(format!(
            "schema too complex: expanded weight {weight} exceeds {MAX_WEIGHT}"
        ));
    }
    graph
        .recursion()
        .map_err(|e| format!("schema too complex: {e}"))
}

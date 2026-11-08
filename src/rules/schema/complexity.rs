use std::collections::{HashMap, VecDeque};

use jsonschema::{Registry, Uri};
use serde_json::Value;

const MAX_WEIGHT: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Storage,
    Same,
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

fn children(value: &Value) -> Vec<(String, &Value, Kind)> {
    let mut children = Vec::new();
    let Some(object) = value.as_object() else {
        return children;
    };
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
    children
}

impl Graph {
    fn build(registry: &Registry<'_>, uri: Uri<String>) -> Result<Self, String> {
        let root = registry
            .resolver(uri)
            .lookup("#")
            .expect("the prepared registry contains the schema root");
        let (value, resolver, draft) = root.into_inner();
        let resolver = resolver
            .in_subresource(draft.create_resource_ref(value))
            .expect("the prepared registry resolved the root identifier");
        let mut graph = Self::default();
        let mut indices = HashMap::new();
        let mut pending = vec![(value, resolver, draft, "#".to_owned())];
        indices.insert(std::ptr::from_ref(value), 0);
        graph.edges.push(Vec::new());
        graph.locations.push("#".to_owned());
        while let Some((value, resolver, draft, location)) = pending.pop() {
            let source = indices[&std::ptr::from_ref(value)];
            let mut targets = Vec::new();
            for (suffix, child, kind) in children(value) {
                let scope = resolver
                    .in_subresource(draft.create_resource_ref(child))
                    .map_err(|e| e.to_string())?;
                targets.push((
                    child,
                    scope,
                    draft,
                    format!("{location}{suffix}"),
                    kind,
                ));
            }
            for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
                if let Some(reference) = value.get(keyword).and_then(Value::as_str) {
                    let target_resource =
                        resolver.lookup(reference).map_err(|e| e.to_string())?;
                    let (child, scope, draft) = target_resource.into_inner();
                    targets.push((
                        child,
                        scope,
                        draft,
                        reference.to_owned(),
                        Kind::Same,
                    ));
                }
            }
            for (child, scope, draft, location, kind) in targets {
                let next = indices.len();
                let target =
                    *indices.entry(std::ptr::from_ref(child)).or_insert_with(|| {
                        graph.edges.push(Vec::new());
                        graph.locations.push(location.clone());
                        pending.push((child, scope, draft, location));
                        next
                    });
                graph.edges[source].push(Edge { target, kind });
            }
        }
        Ok(graph)
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
                Kind::Array => kinds |= 1,
                Kind::Object | Kind::Property => kinds |= 2,
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
        let mut kinds = 0;
        for edge in self.internal(node, components) {
            if edge.kind == Kind::Property {
                if kinds & 2 != 0 {
                    return true;
                }
                properties = true;
                continue;
            }
            let descent = self.first_descent(edge, components);
            if descent & kinds != 0 || (descent & 2 != 0 && properties) {
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
            for edge in edges.iter().filter(|e| e.kind == Kind::Same) {
                incoming[edge.target] += 1;
            }
        }
        let mut ready = incoming
            .iter()
            .enumerate()
            .filter_map(|(n, &count)| (count == 0).then_some(n))
            .collect::<VecDeque<_>>();
        while let Some(node) = ready.pop_front() {
            for edge in self.edges[node].iter().filter(|e| e.kind == Kind::Same) {
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
    let graph = Graph::build(registry, uri)?;
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

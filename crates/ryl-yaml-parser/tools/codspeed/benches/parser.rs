//! Parser benchmarks, run from `tools/codspeed` with `cargo bench --bench parser`
//! or through `CodSpeed`.
//!
//! The inputs are generated deterministically so that results stay comparable across runs.

use std::fmt::Write as _;

use divan::{black_box, Bencher};
#[cfg(feature = "comments")]
use granit_parser::options;
use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

fn main() {
    divan::main();
}

/// A receiver which only counts the events it is sent.
#[derive(Default)]
struct CountingSink {
    events: usize,
}

impl<'input> SpannedEventReceiver<'input> for CountingSink {
    fn on_event(&mut self, event: Event<'input>, span: Span) {
        black_box((&event, &span));
        self.events += 1;
    }
}

/// Parse `input` from a string slice and return the number of emitted events.
fn parse_str(input: &str) -> usize {
    let mut sink = CountingSink::default();
    Parser::new_from_str(input)
        .load(&mut sink, true)
        .expect("benchmark input must be valid YAML");
    sink.events
}

/// A block-style document resembling a service configuration file.
fn block_config(entries: usize) -> String {
    let mut out = String::from("# Generated configuration\nversion: 3\nservices:\n");
    for i in 0..entries {
        let _ = write!(
            out,
            "  service_{i}:\n    image: \"registry.example.com/app:{i}\"\n    replicas: {}\n    \
             enabled: true\n    ports:\n      - 80{:02}\n      - 443\n    env:\n      \
             NAME: service-{i}\n      RATIO: {}.5\n",
            i % 7,
            i % 100,
            i % 13
        );
    }
    out
}

/// A JSON-like document made of nested flow collections.
fn flow_records(records: usize) -> String {
    let mut out = String::from("[\n");
    for i in 0..records {
        let _ = writeln!(
            out,
            "  {{id: {i}, name: \"user {i}\", tags: [a, b, c], score: {}.25, active: {}}},",
            i * 3,
            i % 2 == 0
        );
    }
    out.push_str("]\n");
    out
}

/// A document dominated by multi-line literal and folded block scalars.
fn block_scalars(entries: usize) -> String {
    let mut out = String::new();
    for i in 0..entries {
        let _ = write!(
            out,
            "literal_{i}: |\n  line one of entry {i}\n  line two with some more text\n\n  \
             final line\nfolded_{i}: >-\n  folded text that spans\n  several source lines\n  \
             for entry {i}\n"
        );
    }
    out
}

/// A document with many comments, anchors, aliases, tags and quoted scalars.
fn annotated(entries: usize) -> String {
    let mut out = String::from(
        "%YAML 1.2\n---\n# Header comment\ndefaults: &defaults\n  retries: 3\n  timeout: 30\n",
    );
    for i in 0..entries {
        let _ = write!(
            out,
            "# Entry {i}\nitem_{i}: # trailing comment\n  <<: *defaults\n  id: !!int {i}\n  \
             label: 'single ''quoted'' {i}'\n  note: \"double \\\"quoted\\\" \\u00e9 {i}\"\n  \
             ref: &anchor_{i} value_{i}\n  alias: *anchor_{i}\n"
        );
    }
    out.push_str("...\n");
    out
}

/// A stream made of many small documents.
fn multi_document(documents: usize) -> String {
    let mut out = String::new();
    for i in 0..documents {
        let _ = write!(out, "---\nkind: event\nseq: {i}\npayload: [x, y, z]\n");
    }
    out
}

/// A deeply nested block mapping.
fn deep_nesting(depth: usize) -> String {
    let mut out = String::new();
    for level in 0..depth {
        let _ = writeln!(out, "{:indent$}level_{level}:", "", indent = level * 2);
    }
    let _ = writeln!(out, "{:indent$}leaf: value", "", indent = depth * 2);
    out
}

#[divan::bench(args = [10, 100, 1000])]
fn block_mapping(bencher: Bencher, entries: usize) {
    let input = block_config(entries);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [10, 100, 1000])]
fn flow_collections(bencher: Bencher, records: usize) {
    let input = flow_records(records);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [100])]
fn literal_and_folded_scalars(bencher: Bencher, entries: usize) {
    let input = block_scalars(entries);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [100])]
fn comments_anchors_tags(bencher: Bencher, entries: usize) {
    let input = annotated(entries);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [100])]
#[cfg(feature = "comments")]
fn comments_disabled(bencher: Bencher, entries: usize) {
    let input = annotated(entries);
    bencher.bench(|| {
        let mut sink = CountingSink::default();
        Parser::new_from_str_with_options(black_box(&input), options! { emit_comments: false })
            .load(&mut sink, true)
            .expect("benchmark input must be valid YAML");
        sink.events
    });
}

#[divan::bench(args = [100])]
fn many_documents(bencher: Bencher, documents: usize) {
    let input = multi_document(documents);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [100])]
fn nested_mappings(bencher: Bencher, depth: usize) {
    let input = deep_nesting(depth);
    bencher.bench(|| parse_str(black_box(&input)));
}

#[divan::bench(args = [100])]
fn buffered_char_iterator(bencher: Bencher, entries: usize) {
    let input = block_config(entries);
    bencher.bench(|| {
        let mut sink = CountingSink::default();
        Parser::new_from_iter(black_box(&input).chars())
            .load(&mut sink, true)
            .expect("benchmark input must be valid YAML");
        sink.events
    });
}

#[divan::bench(args = [100])]
fn event_iterator(bencher: Bencher, entries: usize) {
    let input = block_config(entries);
    bencher.bench(|| {
        Parser::new_from_str(black_box(&input))
            .try_fold(0usize, |count, event| event.map(|_| count + 1))
            .expect("benchmark input must be valid YAML")
    });
}

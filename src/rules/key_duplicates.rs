//! `key-duplicates` rule &mdash; reports duplicate mapping keys (issue #252).
//!
//! Default behaviour mirrors yamllint: keys are compared by their literal text,
//! and a duplicate `<<` merge key is silent unless `forbid-duplicated-merge-keys`
//! is set (matching yamllint, which keys this on the resolved scalar value, so a
//! quoted `"<<"` is treated the same as a plain `<<`).
//!
//! Two ryl-only, off-by-default, TOML-only knobs add *semantic* duplicate
//! detection grounded in the YAML 1.2.2 spec, which requires mapping keys to be
//! unique and defines key equality via each tag's canonical form (YAML 1.2.2
//! spec, §3.2.1.3 Node Comparison; adrienverge/yamllint#175 tracks the same gap
//! upstream):
//!
//! * `check-canonical` &mdash; resolve plain scalars under the YAML 1.2 core
//!   schema before comparison, so `0xB`/`011`/`11` (all integer 11) or
//!   `Null`/`~` collide while a quoted `"11"` (a string) stays distinct from the
//!   integer `11`. A key carrying a local / non-core tag falls back to literal
//!   text comparison, since its resolved type is application-defined.
//! * `forbid-merge-key-shadowing` &mdash; additionally reports a key set both by
//!   a merge and explicitly when the two values differ.
//!
//! Either knob enables value-aware **merge-collision** detection: a `<<` that
//! merges mappings assigning *different values* to one key is reported at the
//! `<<` line (unless an explicit key in the host overrides it), because YAML
//! silently keeps the first. Merge collisions are value-aware on purpose: a key
//! contributed twice with the *same* value (the same anchor merged twice, or two
//! anchors that agree) is not a collision &mdash; matching yamllint, which
//! accepts such documents. Only explicit-vs-explicit duplicates stay value-blind
//! (yamllint parity).
//!
//! Each node's value identity ([`Vid`]) is a deterministic 64-bit hash folded
//! over its children (a mapping's hash is order-independent; a non-core tag is
//! folded in; an alias resolves to the anchored node's hash). Hashing &mdash;
//! rather than materialising an alias-expanded value tree &mdash; plus merging
//! each anchor into a host at most once keeps the work linear in the source, so
//! the lint path never reintroduces the alias-expansion blow-up bounded out of
//! the YAML config loader (issue #246). Identities are built only when a merge
//! knob is on; the default path compares key text alone. There is no safe
//! `--fix` (see AGENTS.md "Rules Without A Safe `--fix`").

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use granit_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver, Tag};

use crate::config::YamlLintConfig;
use crate::rules::support::mapping_key_walker::Walker;
use crate::yaml_dom::{Scalar, ScalarOwned, core_schema_suffix, is_core_schema};

pub const ID: &str = "key-duplicates";

#[derive(Debug, Clone, Copy)]
pub struct Config {
    forbid_duplicated_merge_keys: bool,
    check_canonical: bool,
    forbid_merge_key_shadowing: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            forbid_duplicated_merge_keys: cfg.rule_option_bool(
                ID,
                "forbid-duplicated-merge-keys",
                false,
            ),
            check_canonical: cfg.rule_option_bool(ID, "check-canonical", false),
            forbid_merge_key_shadowing: cfg.rule_option_bool(
                ID,
                "forbid-merge-key-shadowing",
                false,
            ),
        }
    }

    const fn expand_merges(self) -> bool {
        self.check_canonical || self.forbid_merge_key_shadowing
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = KeyDuplicatesReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    let mut violations = receiver.violations;
    violations.sort_by_key(|v| (v.line, v.column));
    // One key colliding across several merge sources reports once: the repeated
    // diagnostics share a position and message, so adjacent copies collapse.
    violations.dedup();
    violations
}

/// A mapping key's identity for duplicate comparison: its canonical scalar
/// value when `check-canonical` resolves it, otherwise its literal text (the
/// yamllint default, also used as the fallback for non-core-tagged keys).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyId {
    Resolved(ScalarOwned),
    Raw(String),
}

fn key_id(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Cow<'_, Tag>>,
    canonical: bool,
) -> KeyId {
    if canonical
        && tag.is_none_or(|tag| is_core_schema(tag))
        && let Some(scalar) = Scalar::resolve_scalar(Cow::Borrowed(value), style, tag)
    {
        return KeyId::Resolved(scalar.into_owned());
    }
    KeyId::Raw(value.to_owned())
}

/// A node's value identity: a deterministic hash folded over the node, used to
/// decide whether a merge would silently change a key's value. An alias resolves
/// to the anchored node's `Vid`, so a hash &mdash; not a cloned subtree &mdash;
/// propagates, keeping alias resolution linear. Nested `<<` inside a value is
/// hashed structurally (not re-resolved), which only over-distinguishes deeply
/// nested merges.
type Vid = u64;

const UNKNOWN_VID: Vid = 0;

/// Upper bound on merged key contributions materialised per lint run (per
/// `check` call; an embedded-Markdown region is its own run). Resolving merges
/// materialises one entry per merged key, so a wide anchor referenced many times
/// (`<<: [*base, *base, ...]` across many hosts) would otherwise grow
/// super-linearly; past this cap the merge analysis degrades to no-op rather
/// than exhaust memory (mirrors the YAML loader's alias-expansion bound, #246).
const MAX_MERGE_CONTRIBUTIONS: usize = 1_000_000;

fn hash_of(value: &impl Hash) -> Vid {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// The core schema's default tags (those an untagged node resolves to) don't
/// distinguish a value; any other tag does.
const DEFAULT_TAG_SUFFIXES: [&str; 7] =
    ["map", "seq", "str", "int", "float", "bool", "null"];

/// A tag's contribution to a node's `Vid`: a local tag (`!foo`) or a non-default
/// core tag (`!!set`, `!!omap`) distinguishes the value from an untagged node;
/// the implicit default tags do not.
fn tag_vid(tag: Option<&Cow<'_, Tag>>) -> Vid {
    match tag.map(Cow::as_ref) {
        // `core_schema_suffix`, not granit's handle-only check, so a verbatim core
        // default tag is recognised as non-distinguishing too (#277).
        Some(tag)
            if !matches!(
                core_schema_suffix(tag),
                Some(suffix) if DEFAULT_TAG_SUFFIXES.contains(&suffix)
            ) =>
        {
            hash_of(tag)
        }
        _ => UNKNOWN_VID,
    }
}

fn scalar_vid(value: &str, style: ScalarStyle, tag: Option<&Cow<'_, Tag>>) -> Vid {
    hash_of(&(0u8, key_id(value, style, tag, true), tag_vid(tag)))
}

type Pos = (usize, usize);

/// A merged key's value identity, used to compare what each anchor assigns.
type Contribution = (KeyId, String, Vid);

#[derive(Debug, Clone)]
struct SeenKey {
    id: KeyId,
    text: String,
    value: Vid,
    directive: bool,
    pos: Pos,
}

#[derive(Debug, Clone)]
struct Merged {
    id: KeyId,
    text: String,
    value: Vid,
    pos: Pos,
}

#[derive(Debug, Clone, Copy)]
enum Role {
    Normal,
    MergeValue(Pos),
    MergeElement,
}

struct MapData {
    seen: Vec<SeenKey>,
    merges: Vec<Merged>,
    merged_anchors: HashSet<usize>,
    anchor_id: usize,
    node_tag: Vid,
    role: Role,
    pending_merge: Option<Pos>,
    pending_value_key: Option<usize>,
    expect_value: bool,
    vid_children: Vec<Vid>,
}

impl MapData {
    fn new(anchor_id: usize, node_tag: Vid, role: Role) -> Self {
        Self {
            seen: Vec::new(),
            merges: Vec::new(),
            merged_anchors: HashSet::new(),
            anchor_id,
            node_tag,
            role,
            pending_merge: None,
            pending_value_key: None,
            expect_value: false,
            vid_children: Vec::new(),
        }
    }

    /// This mapping's resolved key set for when it is itself merged elsewhere:
    /// merged keys (first source wins, per YAML merge precedence) overridden by
    /// explicit keys. Carrying merged keys through here is what makes a
    /// transitively-merged base (`&b` built via its own `<<`) propagate.
    fn effective_contributions(&self) -> Vec<Contribution> {
        let mut out: Vec<Contribution> = Vec::new();
        let mut index: HashMap<&KeyId, usize> = HashMap::new();
        for merged in &self.merges {
            index.entry(&merged.id).or_insert_with(|| {
                out.push((merged.id.clone(), merged.text.clone(), merged.value));
                out.len() - 1
            });
        }
        for key in self.seen.iter().filter(|key| !key.directive) {
            if let Some(&i) = index.get(&key.id) {
                out[i].1.clone_from(&key.text);
                out[i].2 = key.value;
            } else {
                index.insert(&key.id, out.len());
                out.push((key.id.clone(), key.text.clone(), key.value));
            }
        }
        out
    }

    /// Order-independent so two mappings that differ only in key order share an
    /// identity (YAML mapping key order is insignificant); a non-core tag still
    /// distinguishes.
    fn vid(&self) -> Vid {
        let mut entries: Vec<Vid> = self
            .vid_children
            .chunks_exact(2)
            .map(|pair| hash_of(&(pair[0], pair[1])))
            .collect();
        entries.sort_unstable();
        hash_of(&(2u8, self.node_tag, entries))
    }
}

#[derive(Debug, Default)]
struct SeqMeta {
    /// `Some` when this sequence is the value of a `<<` key: its merged mappings
    /// flush into the host at the recorded position.
    merge_span: Option<Pos>,
    contributions: Vec<Contribution>,
    merged_anchors: HashSet<usize>,
    vid_items: Vec<Vid>,
    anchor_id: usize,
    node_tag: Vid,
}

impl SeqMeta {
    /// A sequence accumulates merged mappings either because it is a `<<` value
    /// or because it is anchored (so `<<: *seq` can later merge its mappings).
    const fn accumulates(&self) -> bool {
        self.merge_span.is_some() || self.anchor_id != 0
    }
}

struct KeyDuplicatesReceiver<'cfg> {
    state: KeyDuplicatesState<'cfg>,
    violations: Vec<Violation>,
}

impl<'cfg> KeyDuplicatesReceiver<'cfg> {
    fn new(config: &'cfg Config) -> Self {
        Self {
            state: KeyDuplicatesState::new(config),
            violations: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for KeyDuplicatesReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::StreamStart | Event::DocumentStart(_) | Event::DocumentEnd => {
                self.state.reset();
            }
            Event::SequenceStart(_, anchor_id, ref tag) => {
                self.state.enter_sequence(anchor_id, tag.as_ref());
            }
            Event::SequenceEnd => self.state.exit_sequence(),
            Event::MappingStart(_, anchor_id, ref tag) => {
                self.state.enter_mapping(anchor_id, tag.as_ref());
            }
            Event::MappingEnd => self.state.exit_mapping(&mut self.violations),
            Event::Scalar(value, style, anchor_id, tag) => {
                self.state.handle_scalar(
                    value.as_ref(),
                    style,
                    anchor_id,
                    tag.as_ref(),
                    span,
                );
            }
            Event::Alias(anchor_id) => self.state.handle_alias(anchor_id),
            Event::Comment(_, _) | Event::StreamEnd | Event::Nothing => {}
        }
    }
}

struct KeyDuplicatesState<'cfg> {
    config: &'cfg Config,
    walker: Walker<MapData, SeqMeta>,
    anchor_keys: HashMap<usize, Vec<Contribution>>,
    anchor_vids: HashMap<usize, Vid>,
    budget: usize,
    degraded: bool,
}

impl<'cfg> KeyDuplicatesState<'cfg> {
    fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            walker: Walker::new(),
            anchor_keys: HashMap::new(),
            anchor_vids: HashMap::new(),
            budget: MAX_MERGE_CONTRIBUTIONS,
            degraded: false,
        }
    }

    // Anchors are document-scoped, but the merge budget spans the whole lint run
    // (a multi-document payload cannot escape the bound).
    fn reset(&mut self) {
        self.walker.reset();
        self.anchor_keys.clear();
        self.anchor_vids.clear();
    }

    /// Charge `n` merged contributions against the run budget. Once it is
    /// exhausted the run is marked `degraded`: callers skip the materialisation
    /// and `detect` stops reporting merge collisions, because a skipped merge can
    /// change the resolved value and turn a non-collision into a false positive.
    /// Degrading to missed (never spurious) merge diagnostics is the safe choice.
    fn spend(&mut self, n: usize) -> bool {
        if n > self.budget {
            self.degraded = true;
            return false;
        }
        self.budget -= n;
        true
    }

    /// Merge `anchor`'s resolved keys into the host mapping, charging the budget
    /// *before* cloning so an over-budget alias is O(1), not O(keys).
    fn merge_anchor_into_host(&mut self, anchor: usize, pos: Pos) {
        if self.spend(self.anchor_keys.get(&anchor).map_or(0, Vec::len)) {
            let contributions = self.anchor_contributions(anchor);
            let host = self
                .walker
                .current_mapping_mut()
                .expect("a `<<` value's parent is the mapping that declared it");
            push_merges(host, contributions, pos);
        }
    }

    fn merge_anchor_into_seq(&mut self, anchor: usize) {
        if self.spend(self.anchor_keys.get(&anchor).map_or(0, Vec::len)) {
            let contributions = self.anchor_contributions(anchor);
            self.walker
                .current_metadata_mut()
                .expect("a merge element's parent is the sequence")
                .contributions
                .extend(contributions);
        }
    }

    // Inline merge mappings and the sequence flush move content that is already
    // bounded by the source (or already charged during accumulation), so only
    // the alias path above charges the budget — charging here would double-count
    // a sequence's contributions.
    fn merge_into_host(&mut self, contributions: Vec<Contribution>, pos: Pos) {
        let host = self
            .walker
            .current_mapping_mut()
            .expect("a `<<` value's parent is the mapping that declared it");
        push_merges(host, contributions, pos);
    }

    fn merge_into_seq(&mut self, contributions: Vec<Contribution>) {
        self.walker
            .current_metadata_mut()
            .expect("a merge element's parent is the sequence")
            .contributions
            .extend(contributions);
    }

    fn enter_mapping(&mut self, anchor_id: usize, tag: Option<&Cow<'_, Tag>>) {
        let role = self.role_for_child();
        self.walker.enter_mapping(
            MapData::new(anchor_id, tag_vid(tag), role),
            SeqMeta::default(),
        );
    }

    fn enter_sequence(&mut self, anchor_id: usize, tag: Option<&Cow<'_, Tag>>) {
        let merge_span = self
            .walker
            .current_mapping_mut()
            .and_then(|map| map.pending_merge.take());
        self.walker.enter_sequence(SeqMeta {
            merge_span,
            anchor_id,
            node_tag: tag_vid(tag),
            ..SeqMeta::default()
        });
    }

    /// Classify a child node by its parent: the value of a `<<` key, an element
    /// of an accumulating `<<`/anchored sequence, or an ordinary node.
    fn role_for_child(&mut self) -> Role {
        if let Some(span) = self
            .walker
            .current_mapping_mut()
            .and_then(|map| map.pending_merge.take())
        {
            return Role::MergeValue(span);
        }
        if self.walker.current_mapping_mut().is_none()
            && self
                .walker
                .current_metadata_mut()
                .is_some_and(|meta| meta.accumulates())
        {
            return Role::MergeElement;
        }
        Role::Normal
    }

    /// Route a completed node's value identity into its parent container and
    /// register it if anchored. Mapping children alternate key, value; the
    /// value identity backfills the key's `seen` entry.
    fn place_node_vid(&mut self, vid: Vid, anchor_id: usize) {
        if anchor_id != 0 {
            self.anchor_vids.insert(anchor_id, vid);
        }
        if let Some(map) = self.walker.current_mapping_mut() {
            if map.expect_value {
                if let Some(idx) = map.pending_value_key.take() {
                    map.seen[idx].value = vid;
                }
                map.expect_value = false;
            } else {
                map.expect_value = true;
            }
            map.vid_children.push(vid);
        } else if let Some(meta) = self.walker.current_metadata_mut() {
            meta.vid_items.push(vid);
        }
    }

    fn exit_mapping(&mut self, diagnostics: &mut Vec<Violation>) {
        let expand = self.config.expand_merges();
        let reliable = !self.degraded;
        let map = self
            .walker
            .current_mapping_mut()
            .expect("a mapping is open when MappingEnd fires");
        let anchor_id = map.anchor_id;
        let role = map.role;
        // Only the merge-derived keys can amplify: a nested anchored merge
        // re-materialises them at each level. Explicit keys are bounded by the
        // source, so they are not charged (a huge but ordinary anchored document
        // must not degrade). Charging the merge size here (beyond the alias clone)
        // bounds the re-materialisation and trips `degraded`, which suppresses the
        // now-unreliable merge reports.
        let size = map.merges.len();
        detect(map, *self.config, reliable, diagnostics);
        if !expand {
            self.walker.exit_container();
            return;
        }
        // The resolved key set is only consumed when this mapping is reused (as an
        // anchor or a merge value/element).
        let materialise =
            (anchor_id != 0 || !matches!(role, Role::Normal)) && self.spend(size);
        let contributions = if materialise {
            self.open_mapping().effective_contributions()
        } else {
            Vec::new()
        };
        let vid = self.open_mapping().vid();
        if anchor_id != 0 {
            self.anchor_keys.insert(anchor_id, contributions.clone());
        }
        self.walker.exit_container();
        self.place_node_vid(vid, anchor_id);
        match role {
            Role::MergeValue(pos) => self.merge_into_host(contributions, pos),
            Role::MergeElement => self.merge_into_seq(contributions),
            Role::Normal => {}
        }
    }

    fn open_mapping(&mut self) -> &mut MapData {
        self.walker
            .current_mapping_mut()
            .expect("a mapping is open when MappingEnd fires")
    }

    fn exit_sequence(&mut self) {
        if !self.config.expand_merges() {
            self.walker.exit_container();
            return;
        }
        let meta = self
            .walker
            .current_metadata_mut()
            .expect("a sequence is open when SequenceEnd fires");
        let vid = hash_of(&(1u8, meta.node_tag, &meta.vid_items));
        let anchor_id = meta.anchor_id;
        let merge_span = meta.merge_span;
        let contributions = std::mem::take(&mut meta.contributions);
        self.walker.exit_container();
        self.place_node_vid(vid, anchor_id);
        if anchor_id != 0 {
            // Registered so `<<: *seq` merges the sequence's mappings.
            self.anchor_keys.insert(anchor_id, contributions.clone());
        }
        if let Some(span) = merge_span {
            self.merge_into_host(contributions, span);
        }
    }

    fn handle_scalar(
        &mut self,
        value: &str,
        style: ScalarStyle,
        anchor_id: usize,
        tag: Option<&Cow<'_, Tag>>,
        span: Span,
    ) {
        let context = self.walker.begin_node();
        let expand = self.config.expand_merges();
        if !context.key_root() {
            if expand {
                if let Some(map) = self.walker.current_mapping_mut() {
                    map.pending_merge = None;
                }
                self.place_node_vid(scalar_vid(value, style, tag), anchor_id);
            }
            self.walker.finish_node(context);
            return;
        }

        let id = key_id(value, style, tag, self.config.check_canonical);
        let is_merge_directive =
            crate::rules::support::merge_key::is_merge_directive(value, style, tag);
        let pos = (span.start.line(), span.start.col() + 1);
        let map = self
            .walker
            .current_mapping_mut()
            .expect("mapping state should exist when key_root is active");
        let idx = map.seen.len();
        map.seen.push(SeenKey {
            id,
            text: value.to_owned(),
            value: UNKNOWN_VID,
            directive: is_merge_directive,
            pos,
        });

        if expand {
            self.place_node_vid(scalar_vid(value, style, tag), anchor_id);
            let map = self
                .walker
                .current_mapping_mut()
                .expect("mapping state should exist when key_root is active");
            map.pending_value_key = Some(idx);
            if is_merge_directive {
                map.pending_merge = Some(pos);
            }
        }

        self.walker.finish_node(context);
    }

    fn handle_alias(&mut self, anchor_id: usize) {
        let context = self.walker.begin_node();
        if self.config.expand_merges() {
            let vid = self
                .anchor_vids
                .get(&anchor_id)
                .copied()
                .unwrap_or(UNKNOWN_VID);
            if let Some(pos) = self
                .walker
                .current_mapping_mut()
                .and_then(|map| map.pending_merge.take())
            {
                // Single-alias `<<` value; merge each anchor into a host at most
                // once (idempotent) so a repeated alias cannot amplify work.
                if self.host_accepts_anchor(anchor_id) {
                    self.merge_anchor_into_host(anchor_id, pos);
                }
            } else if self.walker.current_metadata_mut().is_some_and(|meta| {
                meta.accumulates() && meta.merged_anchors.insert(anchor_id)
            }) {
                self.merge_anchor_into_seq(anchor_id);
            }
            self.place_node_vid(vid, 0);
        }
        self.walker.finish_node(context);
    }

    fn host_accepts_anchor(&mut self, anchor_id: usize) -> bool {
        self.walker
            .current_mapping_mut()
            .expect("a `<<` value's parent is the mapping that declared it")
            .merged_anchors
            .insert(anchor_id)
    }

    fn anchor_contributions(&self, anchor_id: usize) -> Vec<Contribution> {
        self.anchor_keys
            .get(&anchor_id)
            .cloned()
            .unwrap_or_default()
    }
}

fn push_merges(host: &mut MapData, contributions: Vec<Contribution>, pos: Pos) {
    for (id, text, value) in contributions {
        host.merges.push(Merged {
            id,
            text,
            value,
            pos,
        });
    }
}

fn violation(pos: Pos, text: &str, location: &str) -> Violation {
    Violation {
        line: pos.0,
        column: pos.1,
        message: format!("duplication of key \"{text}\" {location}"),
    }
}

/// Report the duplicates of one mapping: explicit-vs-explicit keys (value-blind,
/// yamllint parity) and, when a merge knob is on, merge-involved collisions that
/// would silently change a key's value.
fn detect(
    map: &MapData,
    cfg: Config,
    reliable: bool,
    diagnostics: &mut Vec<Violation>,
) {
    let mut seen_ids: HashSet<&KeyId> = HashSet::new();
    for key in &map.seen {
        // yamllint exempts a duplicate `<<` (by resolved value, so a quoted
        // `"<<"` too) unless forbid-duplicated-merge-keys is set.
        if !seen_ids.insert(&key.id)
            && (key.text != "<<" || cfg.forbid_duplicated_merge_keys)
        {
            diagnostics.push(violation(key.pos, &key.text, "in mapping"));
        }
    }

    // `reliable` is false once the merge budget is exhausted: a skipped merge can
    // misrepresent a resolved value, so merge-collision reporting is suppressed to
    // avoid false positives (explicit-vs-explicit above is budget-independent).
    if !cfg.expand_merges() || !reliable {
        return;
    }

    let explicit: HashMap<&KeyId, &SeenKey> = map
        .seen
        .iter()
        .filter(|key| !key.directive)
        .map(|key| (&key.id, key))
        .collect();
    let mut first_merge: HashMap<&KeyId, Vid> = HashMap::new();
    let mut reported: HashSet<&KeyId> = HashSet::new();
    for merged in &map.merges {
        if reported.contains(&merged.id) {
            continue;
        }
        if let Some(key) = explicit.get(&merged.id) {
            // An explicit key overrides every merge, so the merges' disagreement
            // is moot; only the explicit-vs-merge shadow (different value) is a
            // reportable change, and only under the strict knob.
            if cfg.forbid_merge_key_shadowing && key.value != merged.value {
                reported.insert(&merged.id);
                if key.pos > merged.pos {
                    diagnostics.push(violation(key.pos, &key.text, "in mapping"));
                } else {
                    diagnostics.push(violation(
                        merged.pos,
                        &merged.text,
                        "in merged mappings",
                    ));
                }
            }
        } else if first_merge
            .get(&merged.id)
            .is_some_and(|value| *value != merged.value)
        {
            reported.insert(&merged.id);
            diagnostics.push(violation(merged.pos, &merged.text, "in merged mappings"));
        } else {
            first_merge.entry(&merged.id).or_insert(merged.value);
        }
    }
}

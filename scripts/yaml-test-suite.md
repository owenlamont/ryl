# yaml-test-suite formatter gate

The ignored `tests/yaml_test_suite.rs` test uses the existing granit-parser dependency.
Normal tests and CI need no network or submodule. Fetch the suite separately:

```sh
git clone --branch data https://github.com/yaml/yaml-test-suite /path/to/suite
git -C /path/to/suite checkout 6ad3d2c62885d82fc349026c136ef560838fdf3d
cargo build --release
YAML_TEST_SUITE=/path/to/suite cargo test --release --test yaml_test_suite -- --ignored --nocapture
```

The gate requires a clean checkout at the pinned commit and exactly 402 cases.
Every case runs through the release CLI with explicit configuration, independent of
user configuration. Valid inputs and formatted output must match `test.event`, and
a second formatting must change no bytes. Invalid cases must remain unchanged with
`format`'s syntax-skip diagnostic, and `check` must exit 1 with a syntax diagnostic.
A syntax skip currently exits 0: refusal is established by the diagnostic and bytes.

Quote style is `preserve` to compare scalar styles directly. The event adapter follows
granit's own suite runner: collection style and document markers are omitted, anchor
names become numeric identities, and untagged empty scalars become granit's `~`.
Scalar content, style, tags, document count, mapping order and aliases remain checked.

`BAIL` rows report deliberate reindent or collection-style refusals after event
preservation and idempotence pass. Reindent causes: `Tab` means tabs prevent safe
movement; `Unfollowable` means the analyzer cannot place a token; `Changed` means the
safety comparison rejects a reindent; `Disabled` means an inline directive prevents
reindent. Collection refusals print their diagnostic reason. Bail-outs count cases,
even when only one document or pass is refused; other passes may still edit the case.
Failures remain failures even when a pass bails out. The gate prints every failure
and bail-out, then counts, and fails when any case fails.

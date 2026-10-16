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

## Recorded release run

Pinned suite: `6ad3d2c62885d82fc349026c136ef560838fdf3d`.
Base ryl: `14ba6e1d0d3473872dad96e8e1246c482c5769f6`, release build, quote style preserved.

| Passed | Failed | Bail-outs | Total |
| --- | --- | --- | --- |
| 360 | 1 | 41 | 402 |

`9C9N`: ryl accepts an under-indented flow sequence marked invalid by the suite;
`format` does not refuse and `check` emits no syntax error. Granit's own suite runner
deliberately accepts the same case for PyYAML/ruamel compatibility. The ryl gate
keeps the reference verdict and fails; no formatter or parser bugs were changed.

Bail-outs passed reference-event preservation and idempotence:

| Case | Reason |
| --- | --- |
| `2JQS` | reindent analyzer cannot place a token |
| `3RLN/02` | tabs prevent safe reindent |
| `3RLN/05` | tabs prevent safe reindent |
| `4ZYM` | tabs prevent safe reindent |
| `5GBF` | tabs prevent safe reindent |
| `6BCT` | tabs prevent safe reindent |
| `6CA3` | tabs prevent safe reindent |
| `6HB6` | tabs prevent safe reindent |
| `6M2F` | reindent analyzer cannot place a token |
| `7A4E` | tabs prevent safe reindent |
| `96NN/00` | tabs prevent safe reindent |
| `96NN/01` | tabs prevent safe reindent |
| `CFD4` | reindent analyzer cannot place a token |
| `DK95/00` | tabs prevent safe reindent |
| `DK95/02` | tabs prevent safe reindent |
| `DK95/03` | tabs prevent safe reindent |
| `DK95/04` | tabs prevent safe reindent |
| `DK95/05` | tabs prevent safe reindent |
| `DK95/07` | tabs prevent safe reindent |
| `DK95/08` | tabs prevent safe reindent |
| `FRK4` | reindent analyzer cannot place a token |
| `HS5T` | tabs prevent safe reindent |
| `J3BT` | tabs prevent safe reindent |
| `M2N8/00` | reindent analyzer cannot place a token |
| `M9B4` | tabs prevent safe reindent |
| `MJS9` | tabs prevent safe reindent |
| `NB6Z` | tabs prevent safe reindent |
| `NHX8` | reindent analyzer cannot place a token |
| `NKF9` | reindent analyzer cannot place a token |
| `PRH3` | tabs prevent safe reindent |
| `Q5MG` | tabs prevent safe reindent |
| `R4YG` | tabs prevent safe reindent |
| `S3PD` | reindent analyzer cannot place a token |
| `SM9W/01` | reindent analyzer cannot place a token |
| `T5N4` | tabs prevent safe reindent |
| `TL85` | tabs prevent safe reindent |
| `UKK6/00` | reindent analyzer cannot place a token |
| `UV7Q` | tabs prevent safe reindent |
| `Y79Y/001` | tabs prevent safe reindent |
| `Y79Y/002` | tabs prevent safe reindent |
| `Y79Y/010` | tabs prevent safe reindent |

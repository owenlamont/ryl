//! The `colons` and `hyphens` fixes on every input shape: the formatter's exact targets,
//! the lint tolerance clamp, and the compact collections both leave alone because their
//! spacing is the collection's indentation.

use ryl::rules::{colons, hyphens};

fn format(input: &str) -> String {
    let once = colons::fix(input, &colons::Config::format())
        .unwrap_or_else(|| input.to_string());
    hyphens::fix(&once, &hyphens::Config::format()).unwrap_or(once)
}

fn left_alone(input: &str) -> Vec<(&'static str, usize, usize)> {
    let colons = colons::unfixed(input, &colons::Config::format())
        .into_iter()
        .map(|hit| (colons::ID, hit.line, hit.column));
    let hyphens = hyphens::unfixed(input, &hyphens::Config::format())
        .into_iter()
        .map(|hit| (hyphens::ID, hit.line, hit.column));
    colons.chain(hyphens).collect()
}

#[test]
fn the_formatter_respaces_every_local_site() {
    for (input, expected) in [
        ("a :  1\n", "a: 1\n"),
        ("\"a\"   :   1\n", "\"a\": 1\n"),
        ("a:      1\nbbb:    2\n", "a: 1\nbbb: 2\n"),
        ("x: &al k\n*al  : v\n", "x: &al k\n*al : v\n"),
        ("&an   : v\n", "&an : v\n"),
        ("!t   : v\n", "!t : v\n"),
        ("&an k  : v\n", "&an k: v\n"),
        ("?   k\n:   v\n", "? k\n: v\n"),
        ("? a\n:   - x\n", "? a\n: - x\n"),
        ("a:   |\n  t\n", "a: |\n  t\n"),
        ("a:   |2\n    t\n", "a: |2\n    t\n"),
        ("a:   foo\n  bar\n", "a: foo\n  bar\n"),
        (
            "f: {a :  1, \"b\"  :2, c:   3}\n",
            "f: {a: 1, \"b\": 2, c: 3}\n",
        ),
        ("f: [a :  1]\n", "f: [a: 1]\n"),
        ("j: {\"a\":1}\n", "j: {\"a\": 1}\n"),
        ("a:\t\t1\n", "a: 1\n"),
        ("a:   &x\n  b: 1\n", "a: &x\n  b: 1\n"),
        ("-   a\n", "- a\n"),
        ("-   |\n  t\n", "- |\n  t\n"),
        ("-   >2\n    t\n", "- >2\n    t\n"),
        ("-   |2\n      t\n", "- |2\n      t\n"),
        ("-   !!str 1\n", "- !!str 1\n"),
        ("-   foo\n  bar\n", "- foo\n  bar\n"),
        ("-   'a\n  b'\n", "- 'a\n  b'\n"),
        ("-   [a,\n  b]\n", "- [a,\n  b]\n"),
        ("-   &a\n  b: 1\n", "- &a\n  b: 1\n"),
        ("-   a: 1\n", "- a: 1\n"),
        ("-   - a\n", "- - a\n"),
        ("- -   n\n", "- - n\n"),
        ("seq:\n  -   a:  1\n  -    b\n", "seq:\n  - a: 1\n  - b\n"),
    ] {
        assert_eq!(format(input), expected, "{input:?}");
        assert_eq!(left_alone(input), [], "{input:?}");
    }
}

#[test]
fn a_compact_collection_continuing_below_is_left_alone_and_reported() {
    for (input, rule) in [
        ("-   a: 1\n    b: 2\n", hyphens::ID),
        ("-   a:\n    b: 1\n", hyphens::ID),
        ("-   a:\n      b: 1\n", hyphens::ID),
        ("-   &a b: 1\n    c: 2\n", hyphens::ID),
        ("-   - a\n    - b\n", hyphens::ID),
        ("-   ? a\n    : b\n", hyphens::ID),
        ("-   a: |2\n      text\n", hyphens::ID),
        ("-   a: \"x\n      y\"\n", hyphens::ID),
        ("?   k1: 1\n    k2: 2\n", colons::ID),
    ] {
        assert_eq!(format(input), input, "{input:?}");
        assert_eq!(left_alone(input), [(rule, 1, 4)], "{input:?}");
    }
    let explicit_value = "? a\n:   - x\n    - y\n";
    assert_eq!(format(explicit_value), explicit_value);
    assert_eq!(left_alone(explicit_value), [(colons::ID, 2, 4)]);
}

#[test]
fn spacing_another_rule_owns_or_that_is_no_indicator_stays_untouched() {
    for input in [
        "a:    # c\n",
        "-   # c\n",
        "e: {a:   }\n",
        "e: [a:  , b]\n",
        "{a:1}\n",
        "- key:value\n",
        "a: |\n  -   x\n  k :  v\n",
        "a: \"x\n  -   y\"\n",
        "- a\n-\n- b\n",
    ] {
        assert_eq!(format(input), input, "{input:?}");
        assert_eq!(left_alone(input), [], "{input:?}");
    }
}

#[test]
fn a_lint_fix_stops_at_the_tolerance_or_the_required_space() {
    for (input, colons, hyphens, expected) in [
        ("a:   1\n", (0, 0), 1, "a: 1\n"),
        ("a   :    1\n", (2, 2), 1, "a  :  1\n"),
        ("*al   : v\n", (0, 1), 1, "*al : v\n"),
        ("?    k\n: v\n", (0, 2), 1, "?  k\n: v\n"),
        ("-   a\n", (0, 1), 0, "- a\n"),
        ("-    a\n", (0, 1), 2, "-  a\n"),
    ] {
        let colons_cfg = colons::Config::new(colons.0, colons.1);
        let hyphens_cfg = hyphens::Config::new(hyphens);
        let once = colons::fix(input, &colons_cfg).unwrap_or_else(|| input.to_string());
        let fixed = hyphens::fix(&once, &hyphens_cfg).unwrap_or(once);
        assert_eq!(fixed, expected, "{input:?} under {colons:?}, {hyphens}");
    }
}

#[test]
fn a_lint_fix_leaves_what_the_tolerance_accepts() {
    let lenient = colons::Config::new(0, 1);
    for input in ["a:\t1\n", "a: 1\n", "e: {a:   }\n"] {
        assert_eq!(colons::fix(input, &lenient), None, "{input:?}");
    }
    let disabled = colons::Config::new(-1, -1);
    assert_eq!(colons::fix("a  :   1\n", &disabled), None);
    assert_eq!(hyphens::fix("-   a\n", &hyphens::Config::new(-1)), None);
    assert_eq!(
        colons::check("e: {a:   }\n", &lenient).len(),
        1,
        "the check keeps flagging what only another rule may fix"
    );
}

#[test]
fn the_formatter_check_lists_only_what_its_fix_rewrites() {
    let input = "? a\n:   - x\n    - y\n-   a: 1\n    b: 2\n";
    assert_eq!(colons::check(input, &colons::Config::format()), []);
    assert_eq!(hyphens::check(input, &hyphens::Config::format()), []);
    assert_eq!(colons::check(input, &colons::Config::new(0, 1)).len(), 1);
    assert_eq!(hyphens::check(input, &hyphens::Config::new(1)).len(), 1);
}

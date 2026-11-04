# /// script
# requires-python = ">=3.15"
# dependencies = ["pytest", "typer", "markdown-it-py", "py-yaml12"]
# ///

from pathlib import Path
import sys
from typing import Any

from markdown_it import MarkdownIt
import pytest
from yaml12 import parse_yaml


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

from scaling_check import CLEAN, markdown, markdown_regions, MIB, repeat, Series, series


@pytest.fixture
def targeted_series() -> dict[str, Series]:
    return {
        spec.name: spec
        for spec in series("ryl")
        if spec.name
        in {
            "format prose-wrap always",
            "format prose-wrap never",
            "format keep-quotes double",
            "format many-ignores",
        }
    }


def yaml_regions(source: str) -> list[str]:
    return [
        token.content
        for token in MarkdownIt().parse(source)
        if token.type == "fence" and token.info == "yaml"
    ]


def test_markdown_clean_witness_keeps_children_with_root_keys() -> None:
    source = repeat(CLEAN, 20_000)
    regions = yaml_regions(markdown(source))
    assert regions == [source]
    assert parse_yaml(regions[0]) == parse_yaml(source)


def test_markdown_keeps_cross_line_anchor_alias_in_one_stream() -> None:
    source = "base: &base [alpha, beta]\n" + repeat(CLEAN, 4_000) + "copy: *base\n"
    regions = yaml_regions(markdown(source))
    assert regions == [source]
    loaded = parse_yaml(regions[0])
    assert loaded["base"] == loaded["copy"] == ["alpha", "beta"]


@pytest.mark.parametrize(
    "ending",
    [pytest.param("", id="missing newline"), pytest.param("\n", id="final newline")],
)
def test_markdown_separates_closing_fence_from_last_yaml_line(ending: str) -> None:
    regions = yaml_regions(markdown("key: value" + ending))
    assert regions == ["key: value\n"]
    assert parse_yaml(regions[0]) == {"key": "value"}


def test_markdown_preserves_block_scalar_trailing_blank_lines() -> None:
    source = "message: |+\n  text\n\n\n"
    regions = yaml_regions(markdown(source))
    assert regions == [source]
    assert parse_yaml(regions[0]) == {"message": "text\n\n\n"}


@pytest.mark.parametrize(
    ("size_bytes", "minimum_regions"),
    [
        pytest.param(1, 1, id="small"),
        pytest.param(8_000, 2, id="many complete regions"),
    ],
)
def test_many_regions_are_independently_valid_and_cover_requested_size(
    *, size_bytes: int, minimum_regions: int
) -> None:
    document = markdown_regions(size_bytes)
    regions = yaml_regions(document)
    loaded = [parse_yaml(region) for region in regions]
    assert len(document.encode()) >= size_bytes
    assert len(regions) >= minimum_regions
    assert all(value == loaded[0] for value in loaded)
    assert loaded[0]["key_0"]["list"] == ["a", "b"]


@pytest.mark.parametrize(
    "name",
    [
        pytest.param("format prose-wrap always", id="prose always"),
        pytest.param("format prose-wrap never", id="prose never"),
        pytest.param("format keep-quotes double", id="retained quotes"),
        pytest.param("format many-ignores", id="native ignores and neighbors"),
    ],
)
def test_targeted_profiles_generate_valid_yaml(
    name: str, targeted_series: dict[str, Series]
) -> None:
    spec = targeted_series[name]
    sources = spec.inputs(4_000 / MIB)
    loaded: list[Any] = [parse_yaml(source) for source in sources.values()]
    assert len(loaded) == 1
    assert loaded[0]
    assert spec.command == ("ryl", "format", "--no-cache")


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-q"]))

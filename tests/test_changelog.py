# /// script
# requires-python = ">=3.10"
# dependencies = ["pytest", "markdown-it-py", "typer"]
# ///

from pathlib import Path
import sys
from typing import Final

import pytest


sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))

import changelog


CHANGELOG: Final = """# Changelog

## Unreleased

New work.

## v0.2.0 - 2025-09-10

No user-facing changes.

## v0.1.0 - 2025-09-07

### Added

- Parse YAML.
"""


def test_release_notes_extract_only_the_exact_version() -> None:
    assert changelog.release_notes(CHANGELOG, "v0.2.0") == "No user-facing changes.\n"
    assert changelog.release_notes(CHANGELOG, "v0.1.0") == (
        "### Added\n\n- Parse YAML.\n"
    )


@pytest.mark.parametrize(
    ("source", "version"),
    [
        pytest.param(CHANGELOG, "v0.3.0", id="missing version"),
        pytest.param(
            "## v0.1.0\n\n## v0.1.0\n\nDuplicate.\n", "v0.1.0", id="duplicate version"
        ),
        pytest.param("## v0.1.0\n\n## v0.2.0\n\nNext.\n", "v0.1.0", id="empty section"),
        pytest.param(
            "## v0.1.0\n\n### Added\n\n## v0.2.0\n\nNext.\n",
            "v0.1.0",
            id="heading without content",
        ),
        pytest.param(
            "## v0.1.0\n\n<!-- TODO -->\n\n## v0.2.0\n\nNext.\n",
            "v0.1.0",
            id="comment without content",
        ),
        pytest.param(
            "## v0.1.0\n\n### Added\n\n- <!-- TODO -->\n",
            "v0.1.0",
            id="comment-only list item",
        ),
        pytest.param("## v0.1.0\n\n```text\n```\n", "v0.1.0", id="empty code fence"),
    ],
)
def test_missing_duplicate_and_empty_sections_fail(source: str, version: str) -> None:
    with pytest.raises(ValueError, match="changelog section"):
        changelog.release_notes(source, version)


def test_draft_groups_merged_pr_titles_by_prefix() -> None:
    subjects = [
        "feat(cli): add format (#123)",
        "fix: preserve endings (#124)",
        "ci: update runner (#125)",
        "Merge pull request #126 from branch",
    ]
    assert changelog.draft_entries(subjects) == {
        "feat": ["add format ([#123](https://github.com/owenlamont/ryl/pull/123))"],
        "fix": [
            "preserve endings ([#124](https://github.com/owenlamont/ryl/pull/124))"
        ],
        "ci": ["update runner ([#125](https://github.com/owenlamont/ryl/pull/125))"],
    }


def test_a_heading_inside_a_code_fence_does_not_split_release_notes() -> None:
    source = "## v0.2.0\n\n```md\n## v0.1.0\n```\n\nReal notes.\n\n## v0.1.0\n\nOld.\n"
    assert changelog.release_notes(source, "v0.2.0") == (
        "```md\n## v0.1.0\n```\n\nReal notes.\n"
    )


def test_nonempty_code_fence_is_release_content() -> None:
    source = "## v0.2.0\n\n```md\n## v0.1.0\n```\n\n## v0.1.0\n\nOld.\n"
    assert changelog.release_notes(source, "v0.2.0") == "```md\n## v0.1.0\n```\n"

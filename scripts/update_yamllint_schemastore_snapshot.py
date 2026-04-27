#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# ///

from __future__ import annotations

import json
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import urlopen


DEFAULT_URL = (
    "https://raw.githubusercontent.com/SchemaStore/schemastore/master/"
    "src/schemas/json/yamllint.json"
)
SNAPSHOT_PATH = (
    Path(__file__).resolve().parents[1]
    / "tests"
    / "fixtures"
    / "schemastore-yamllint.json"
)


def fetch_json(url: str) -> object:
    """Fetch a JSON document from an allowed HTTPS source URL.

    Returns:
        The decoded JSON payload from the remote endpoint.

    Raises:
        SystemExit: If the provided URL does not use HTTPS.
    """
    parsed = urlparse(url)
    if parsed.scheme != "https":
        raise SystemExit(f"unsupported URL scheme for snapshot fetch: {url}")
    with urlopen(url) as response:  # noqa: S310 - scheme is restricted to https above
        return json.load(response)


def main() -> None:
    """Refresh the checked-in SchemaStore yamllint schema snapshot."""
    output = SNAPSHOT_PATH.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    data = fetch_json(DEFAULT_URL)
    output.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    print(f"Updated {output}")


if __name__ == "__main__":
    main()

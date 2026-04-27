#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# ///

from __future__ import annotations

import json
from pathlib import Path


SCHEMASTORE_ID = "https://json.schemastore.org/ryl.json"
SCHEMASTORE_META_SCHEMA = "http://json-schema.org/draft-07/schema#"
SOURCE_PATH = Path(__file__).resolve().parents[1] / "ryl.toml.schema.json"


def transform_schema(node: object) -> object:
    """Convert the checked-in TOML schema into SchemaStore's draft-07 shape.

    Args:
        node: Any JSON value from the checked-in TOML schema artifact.

    Returns:
        The transformed JSON value with SchemaStore-compatible definitions,
        references, and metadata-sensitive fields.
    """
    if isinstance(node, dict):
        transformed: dict[str, object] = {}
        for key, value in node.items():
            if key == "format" and value == "int64":
                continue
            target_key = "definitions" if key == "$defs" else key
            target_value = transform_schema(value)
            if target_key == "$ref" and isinstance(target_value, str):
                target_value = target_value.replace("#/$defs/", "#/definitions/", 1)
            transformed[target_key] = target_value
        return transformed
    if isinstance(node, list):
        return [transform_schema(value) for value in node]
    return node


def main() -> None:
    """Print the SchemaStore-compatible TOML schema to stdout.

    Raises:
        SystemExit: If the transformed schema root is not a JSON object.
    """
    transformed = transform_schema(json.loads(SOURCE_PATH.read_text()))
    if not isinstance(transformed, dict):
        raise SystemExit("transformed schema root must be an object")
    transformed["$schema"] = SCHEMASTORE_META_SCHEMA
    transformed["$id"] = SCHEMASTORE_ID
    print(json.dumps(transformed, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()

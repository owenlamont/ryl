#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import tarfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent
PLATFORMS_PATH = ROOT / "npm-platforms.json"
PACKAGE_JSON_PATH = ROOT / "package.json"
README_PATH = ROOT / "README.md"
LICENSE_PATH = ROOT / "LICENSE"
LAUNCHER_PATH = ROOT / "bin" / "ryl.js"


def load_json(path: Path) -> object:
    """Load UTF-8 JSON from disk.

    Returns:
        Parsed JSON data.
    """

    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, data: object) -> None:
    """Write JSON with stable formatting and a trailing newline."""

    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(data, handle, indent=2)
        handle.write("\n")


def clean_dir(path: Path) -> None:
    """Replace a directory with an empty version of itself."""

    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def copy_text_asset(src: Path, dst: Path) -> None:
    """Copy a shared repo asset into a generated package directory."""

    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)


def extract_archive(
    archive_path: Path, binary_name: str, destination_path: Path
) -> None:
    """Extract one named binary from a zip or tar.gz release asset.

    Raises:
        ValueError: The archive type is unsupported or does not contain exactly
            one matching file entry.
    """

    destination_path.parent.mkdir(parents=True, exist_ok=True)
    if archive_path.suffix == ".zip":
        with zipfile.ZipFile(archive_path) as archive:
            members = [
                name
                for name in archive.namelist()
                if name.rstrip("/").endswith(binary_name)
            ]
            if len(members) != 1:
                raise ValueError(
                    "Expected exactly one "
                    f"'{binary_name}' entry in {archive_path.name}, "
                    f"found {members!r}"
                )
            with (
                archive.open(members[0]) as source,
                destination_path.open("wb") as destination,
            ):
                shutil.copyfileobj(source, destination)
        return

    if archive_path.suffixes[-2:] == [".tar", ".gz"]:
        with tarfile.open(archive_path, "r:gz") as archive:
            members = [
                member
                for member in archive.getmembers()
                if Path(member.name).name == binary_name
            ]
            if len(members) != 1:
                raise ValueError(
                    "Expected exactly one "
                    f"'{binary_name}' entry in {archive_path.name}, "
                    f"found {[m.name for m in members]!r}"
                )
            extracted = archive.extractfile(members[0])
            if extracted is None:
                raise ValueError(
                    f"Archive entry {members[0].name!r} in "
                    f"{archive_path.name} is not a file"
                )
            with extracted, destination_path.open("wb") as destination:
                shutil.copyfileobj(extracted, destination)
        return

    raise ValueError(f"Unsupported archive type: {archive_path.name}")


def build_meta_package(
    out_dir: Path,
    package_data: dict[str, object],
    platforms_data: dict[str, object],
    local_test: bool,
) -> None:
    """Generate the staged meta-package directory."""

    meta_dir = out_dir / "ryl"
    clean_dir(meta_dir)

    copy_text_asset(README_PATH, meta_dir / "README.md")
    copy_text_asset(LICENSE_PATH, meta_dir / "LICENSE")
    copy_text_asset(LAUNCHER_PATH, meta_dir / "bin" / "ryl.js")
    copy_text_asset(PLATFORMS_PATH, meta_dir / "npm-platforms.json")

    meta_package = dict(package_data)
    optional_dependencies: dict[str, str] = {}
    for platform in platforms_data["platforms"]:
        if local_test:
            optional_dependencies[platform["packageName"]] = (
                f"file:../{platform['folderName']}"
            )
        else:
            optional_dependencies[platform["packageName"]] = str(
                package_data["version"]
            )
    meta_package["optionalDependencies"] = optional_dependencies

    write_json(meta_dir / "package.json", meta_package)


def build_platform_packages(
    out_dir: Path,
    package_data: dict[str, object],
    platforms_data: dict[str, object],
    assets_dir: Path,
) -> None:
    """Generate staged package directories for each supported platform.

    Raises:
        FileNotFoundError: A required release asset is missing.
    """

    for platform in platforms_data["platforms"]:
        package_dir = out_dir / platform["folderName"]
        clean_dir(package_dir)

        archive_path = assets_dir / platform["archiveName"]
        if not archive_path.exists():
            raise FileNotFoundError(f"Missing release asset {archive_path}")

        copy_text_asset(README_PATH, package_dir / "README.md")
        copy_text_asset(LICENSE_PATH, package_dir / "LICENSE")
        binary_path = package_dir / "bin" / platform["binaryName"]
        extract_archive(archive_path, platform["binaryName"], binary_path)
        if platform["binaryName"] == "ryl":
            binary_path.chmod(0o755)

        platform_package = {
            "name": platform["packageName"],
            "version": package_data["version"],
            "description": package_data["description"],
            "author": package_data["author"],
            "license": package_data["license"],
            "homepage": package_data["homepage"],
            "bugs": package_data["bugs"],
            "repository": package_data["repository"],
            "type": "commonjs",
            "files": ["bin/", "README.md", "LICENSE"],
            "os": platform["os"],
            "cpu": platform["cpu"],
            "publishConfig": {"access": "public"},
            "preferUnplugged": True,
        }
        write_json(package_dir / "package.json", platform_package)


def main() -> None:
    """Build staged npm packages from GitHub release assets."""

    parser = argparse.ArgumentParser(description="Build npm meta and platform packages")
    parser.add_argument(
        "--assets-dir",
        required=True,
        help="Directory containing release asset archives",
    )
    parser.add_argument(
        "--out-dir",
        required=True,
        help="Directory to write generated npm packages into",
    )
    parser.add_argument(
        "--local-test",
        action="store_true",
        help=(
            "Generate the meta package with file: optionalDependencies "
            "for local install validation"
        ),
    )
    args = parser.parse_args()

    assets_dir = Path(args.assets_dir).resolve()
    out_dir = Path(args.out_dir).resolve()
    package_data = load_json(PACKAGE_JSON_PATH)
    platforms_data = load_json(PLATFORMS_PATH)

    clean_dir(out_dir)
    build_platform_packages(out_dir, package_data, platforms_data, assets_dir)
    build_meta_package(out_dir, package_data, platforms_data, args.local_test)


if __name__ == "__main__":
    main()

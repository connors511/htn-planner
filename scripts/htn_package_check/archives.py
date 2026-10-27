"""Validate and extract immutable Cargo archives, without executing their contents."""

import hashlib
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inherited(value):
    if isinstance(value, dict):
        return value.get("workspace") is True or any(
            inherited(v) for v in value.values()
        )
    if isinstance(value, list):
        return any(inherited(v) for v in value)
    return False


def inspect_archive(archive, destination, expected):
    destination.mkdir(parents=True, exist_ok=True)
    hashes = {}
    roots = set()
    with tarfile.open(archive, "r:gz") as stream:
        members = stream.getmembers()
        for member in members:
            path = PurePosixPath(member.name)
            if (
                path.is_absolute()
                or ".." in path.parts
                or not path.parts
                or "\\" in member.name
            ):
                raise ValueError("unsafe archive path")
            if not member.isfile() and not member.isdir():
                raise ValueError("archive links/special files are forbidden")
            roots.add(path.parts[0])
            if member.isfile():
                if member.name in hashes:
                    raise ValueError("duplicate archive member")
                hashes[member.name] = None
        if len(roots) != 1:
            raise ValueError("archive must have one package root")
        for member in members:
            if member.isfile():
                target = destination / member.name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(stream.extractfile(member).read())
                hashes[member.name] = digest(target)
    root = destination / next(iter(roots))
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    package = manifest["package"]
    if package["name"] != expected or root.name != f"{expected}-{package['version']}":
        raise ValueError("archive package identity mismatch")
    if inherited(manifest):
        raise ValueError("unresolved workspace inheritance")
    check_dependencies(manifest)
    required = [manifest.get("lib", {}).get("path", "src/lib.rs")]
    if isinstance(package.get("readme"), str):
        required.append(package["readme"])
    for kind in ("example", "bin", "test", "bench"):
        required.extend(
            item["path"] for item in manifest.get(kind, []) if "path" in item
        )
    for filename in required:
        require_file(root, root / filename)
    for rust in root.rglob("*.rs"):
        for filename in re.findall(
            r'(?:include_str!|include_bytes!|include!)\s*\(\s*"([^"\n]+)"',
            rust.read_text(),
        ):
            require_file(root, rust.parent / filename)
        for filename in re.findall(r'#\[path\s*=\s*"([^"\n]+)"\]', rust.read_text()):
            require_file(root, rust.parent / filename)
    if expected == "htn-planner-ron":
        dependency = manifest.get("dependencies", {}).get("htn-planner", {})
        if not isinstance(dependency, dict) or not dependency.get("version"):
            raise ValueError("frontend runtime dependency must be versioned")
        require_file(root, root / "examples/warehouse.rs")
    return {
        "name": expected,
        "version": package["version"],
        "root": str(root),
        "sha256": digest(archive),
        "members": hashes,
    }


def require_file(root, path):
    if not path.resolve().is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError("missing or escaping packaged source/documentation file")


def verify_unchanged(receipt):
    parent = Path(receipt["root"]).parent
    actual = {
        str(path.relative_to(parent)): digest(path)
        for path in Path(receipt["root"]).rglob("*")
        if path.is_file()
    }
    if actual != receipt["members"]:
        raise ValueError("extracted archive changed during qualification")


def check_dependencies(table):
    for key, value in table.items():
        if key in ("dependencies", "build-dependencies", "dev-dependencies"):
            for dependency in value.values():
                if isinstance(dependency, dict) and (
                    "path" in dependency or "git" in dependency
                ):
                    raise ValueError(
                        "unresolved local/git dependency in normalized manifest"
                    )
        elif isinstance(value, dict):
            check_dependencies(value)

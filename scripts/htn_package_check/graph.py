"""Resolve graph assertions independent of current transitive package counts."""

from pathlib import Path


def validate_metadata(metadata, allowed_manifests, consumer, runtime_only):
    if Path(metadata["workspace_root"]).resolve() != consumer.resolve():
        raise ValueError("consumer escaped its standalone workspace")
    allowed = {Path(path).resolve() for path in allowed_manifests}
    forbidden = ("bevy",)
    parsers = {
        "htn-planner-ron",
        "ron",
        "serde_json",
        "serde_yaml",
        "toml",
        "pest",
        "nom",
    }
    for package in metadata["packages"]:
        name = package["name"]
        if name.startswith(forbidden) or (runtime_only and name in parsers):
            raise ValueError(f"forbidden consumer dependency: {name}")
        source = package.get("source")
        if source is None:
            if Path(package["manifest_path"]).resolve() not in allowed:
                raise ValueError(
                    "consumer resolves checkout/unrecorded path dependency"
                )
        elif source != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("consumer resolves non-public registry/git dependency")

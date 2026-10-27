#!/usr/bin/env python3
"""Process-boundary test double: never launches a compiler or network client."""

import json
import os
import shutil
from pathlib import Path
import sys
import tomllib

config = json.loads(Path(__file__).with_suffix(".json").read_text())
args = sys.argv[1:]
if args == ["--version"]:
    print("fake Cargo (process regression fixture)")
    sys.exit(0)
if args[0] == "package":
    name = args[args.index("-p") + 1]
    if config.get("fail") == "package-frontend" and name == "htn-planner-ron":
        sys.exit(29)
    output = Path(os.environ["CARGO_TARGET_DIR"]) / "package" / (name + "-0.1.0.crate")
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(config["archives"][name], output)
    print("synthetic Cargo archive produced")
    sys.exit(0)
manifest = tomllib.loads(Path("Cargo.toml").read_text())
name = manifest["package"]["name"]
target = Path(os.environ["CARGO_TARGET_DIR"])
if args[0] == "generate-lockfile":
    Path("Cargo.lock").write_text("# synthetic process fixture lock\n")
elif args[0] == "build":
    wasm = "--target" in args
    if config.get("fail") == "wasm" and wasm:
        sys.exit(23)
    if wasm:
        output = (
            target
            / "wasm32-unknown-unknown/debug"
            / (name.replace("-", "_") if name == "identity-consumer" else name)
        )
        output = output.with_suffix(".wasm")
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(b"\0asm\x01\0\0\0")
    else:
        output = target / "debug" / name
        output.parent.mkdir(parents=True, exist_ok=True)
        status = 17 if config.get("fail") == "native" else 0
        output.write_text(
            "#!/bin/sh\nprintf '%s\\n' '" + "ab" * 32 + f"'\nexit {status}\n"
        )
        output.chmod(0o755)
elif args[0] == "metadata":
    packages = [
        {
            "name": name,
            "manifest_path": str(Path("Cargo.toml").resolve()),
            "source": None,
        }
    ]
    for dependency, settings in manifest.get("dependencies", {}).items():
        packages.append(
            {
                "name": dependency,
                "manifest_path": str(Path(settings["path"]) / "Cargo.toml"),
                "source": None,
            }
        )
    if config.get("fail") == "graph":
        packages.append(
            {
                "name": "bevy_render",
                "manifest_path": "/registry/bevy/Cargo.toml",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
            }
        )
    if config.get("fail") == "parser":
        packages.append(
            {
                "name": "ron",
                "manifest_path": "/registry/ron/Cargo.toml",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
            }
        )
    if config.get("fail") == "external":
        packages.append(
            {
                "name": "unrecorded",
                "manifest_path": str(Path.cwd().parent / "unrecorded/Cargo.toml"),
                "source": None,
            }
        )
    if config.get("fail") == "checkout":
        packages.append(
            {"name": "hidden", "manifest_path": "/checkout/Cargo.toml", "source": None}
        )
    if config.get("fail") == "registry":
        packages.append(
            {
                "name": "hidden",
                "manifest_path": "/private/Cargo.toml",
                "source": "registry+https://private.invalid/index",
            }
        )
    print(json.dumps({"workspace_root": str(Path.cwd()), "packages": packages}))
elif args[0] == "tree":
    print("synthetic process fixture graph")
else:
    raise SystemExit("unexpected fake Cargo command")

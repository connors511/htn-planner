"""Serial offline package production and standalone consumption, with durable receipts."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
from .archives import digest, inspect_archive, verify_unchanged
from .graph import validate_metadata

ROOT = Path(__file__).resolve().parents[2]
LANES = (
    "prerequisites",
    "archives",
    "runtime-native",
    "frontend-native",
    "runtime-wasm-compile",
    "frontend-wasm-compile",
    "identity-node-wasm",
    "dependency-isolation",
    "immutable-packages",
    "source-stability",
)


def source_receipt():
    paths = [
        ROOT / name
        for name in (
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            "scripts/check-htn-packages.py",
        )
    ]
    for folder in (
        "crates/htn-planner",
        "crates/htn-planner-ron",
        "scripts/htn-package-consumers",
        "scripts/htn_package_check",
    ):
        paths.extend(
            p
            for p in (ROOT / folder).rglob("*")
            if p.is_file() and "__pycache__" not in p.parts
        )
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(paths)}


def require_isolated(path, check_cargo_config):
    for ancestor in (path, *path.parents):
        if (ancestor / ".git").exists():
            raise ValueError("external consumer/cache has a Git checkout ancestor")
        if check_cargo_config and any(
            (ancestor / ".cargo" / name).is_file() for name in ("config", "config.toml")
        ):
            raise ValueError("external consumer has an ancestor Cargo config")


class Run:
    def __init__(self, args):
        self.args = args
        self.output = args.output.resolve()
        self.external = Path(
            tempfile.mkdtemp(prefix="htn-package-consumers-", dir=args.external_parent)
        ).resolve()
        require_isolated(self.external, check_cargo_config=True)
        self.target = (args.build_cache or self.external / "build-target").resolve()
        require_isolated(self.target, check_cargo_config=False)
        self.summary = {
            "lanes": {lane: "not-run" for lane in LANES},
            "external_root": str(self.external),
            "target": str(self.target),
            "commands": [],
            "profile": "dev, debug=0",
            "offline": True,
            "archive_production": "supplied archives (origin recorded separately)"
            if args.archives
            else "Cargo package from recorded source",
            "packaging_verifier": "skipped (--no-verify); external consumers verify artifacts",
        }
        self.lane = "prerequisites"
        home = self.external / "cargo-home"
        home.mkdir()
        cached = (
            Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "registry"
        )
        if cached.exists():
            (home / "registry").symlink_to(cached.resolve(), target_is_directory=True)
        self.env = {
            key: value
            for key, value in os.environ.items()
            if key in ("PATH", "HOME", "RUSTUP_HOME", "SYSTEMROOT", "TMPDIR")
        }
        self.env.update(
            CARGO_HOME=str(home),
            CARGO_TARGET_DIR=str(self.target),
            CARGO_BUILD_JOBS="2",
            CARGO_NET_OFFLINE="true",
            CARGO_PROFILE_DEV_DEBUG="0",
            RUSTC_WRAPPER="",
            RUSTC_WORKSPACE_WRAPPER="",
            RUSTUP_TOOLCHAIN=tomllib.loads((ROOT / "rust-toolchain.toml").read_text())[
                "toolchain"
            ]["channel"],
        )

        # Workspace package metadata also resolves unrelated public Git declarations.
        # This home is never used to compile/resolve standalone consumers.
        packaging_home = self.external / "packaging-cargo-home"
        packaging_home.mkdir()
        if cached.exists():
            (packaging_home / "registry").symlink_to(
                cached.resolve(), target_is_directory=True
            )
        git_cache = cached.parent / "git"
        if git_cache.exists():
            (packaging_home / "git").symlink_to(
                git_cache.resolve(), target_is_directory=True
            )
        self.packaging_env = dict(self.env, CARGO_HOME=str(packaging_home))
        self.summary["packaging_cache"] = {
            "public_git_cache_for_workspace_metadata_only": git_cache.exists(),
            "consumer_home_has_git_cache": False,
            "frontend_package_runtime_patch": str(ROOT / "crates/htn-planner"),
            "consumer_runtime_patch": "extracted archive only",
            "fetch": False,
        }

    def command(self, label, command, cwd, env=None):
        environment = self.env if env is None else env
        result = subprocess.run(
            [str(arg) for arg in command],
            cwd=cwd,
            env=environment,
            text=True,
            capture_output=True,
            check=False,
        )
        log = self.output / f"{len(self.summary['commands']):02d}-{label}.log"
        log.write_text(result.stdout + result.stderr)
        self.summary["commands"].append(
            {
                "argv": [str(arg) for arg in command],
                "cwd": str(cwd),
                "exit_code": result.returncode,
                "log": str(log),
                "overrides": {
                    key: environment[key]
                    for key in (
                        "CARGO_HOME",
                        "CARGO_TARGET_DIR",
                        "CARGO_BUILD_JOBS",
                        "CARGO_NET_OFFLINE",
                        "CARGO_PROFILE_DEV_DEBUG",
                        "RUSTC_WRAPPER",
                        "RUSTC_WORKSPACE_WRAPPER",
                        "RUSTUP_TOOLCHAIN",
                    )
                    if key in environment
                },
            }
        )
        self.save()
        if result.returncode:
            raise ValueError(f"{label} failed (exit {result.returncode}); see {log}")
        return result.stdout

    def retain(self, source, label):
        destination = self.output / "artifacts" / label
        destination.parent.mkdir(exist_ok=True)
        shutil.copy2(source, destination)
        return str(destination)

    def cargo(self, label, arguments, cwd):
        return self.command(label, [self.args.cargo, *arguments], cwd)

    def save(self):
        (self.output / "summary.json").write_text(
            json.dumps(self.summary, indent=2) + "\n"
        )

    def execute(self):
        try:
            self.qualify()
        except (OSError, ValueError, KeyError) as error:
            self.summary["lanes"][self.lane] = "fail"
            self.summary["error"] = str(error)
            self.save()
            raise

    def qualify(self):
        before = source_receipt()
        self.summary["source"] = {
            "head": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
            ).strip(),
            "dirty": bool(
                subprocess.check_output(
                    ["git", "status", "--porcelain"], cwd=ROOT, text=True
                )
            ),
            "files": before,
        }
        if self.summary["source"]["dirty"] and not self.args.allow_dirty:
            raise ValueError(
                "dirty source requires --allow-dirty and is recorded explicitly"
            )
        self.summary["tools"] = {
            tool: self.command(
                tool + "-version", [executable, "--version"], self.external
            ).strip()
            for tool, executable in (
                ("cargo", self.args.cargo),
                ("rustc", "rustc"),
                ("node", self.args.node),
            )
        }
        self.summary["tools"]["rustc_verbose"] = self.command(
            "rustc-platform", ["rustc", "-vV"], self.external
        )
        self.host = next(
            (
                line.split(": ", 1)[1]
                for line in self.summary["tools"]["rustc_verbose"].splitlines()
                if line.startswith("host: ")
            ),
            None,
        )
        if not self.host:
            raise ValueError("rustc did not report a native host target")
        self.summary["lanes"]["prerequisites"] = "pass"
        self.lane = "archives"
        packages = {}
        for index, name in enumerate(("htn-planner", "htn-planner-ron")):
            if self.args.archives:
                original = self.args.archives[index].resolve()
            else:
                arguments = [
                    "package",
                    "-p",
                    name,
                    "--no-verify",
                    "--offline",
                    "--locked",
                ]
                if self.args.allow_dirty:
                    arguments.append("--allow-dirty")
                if index:
                    arguments += [
                        "--config",
                        f"patch.crates-io.htn-planner.path={json.dumps(str(ROOT / 'crates/htn-planner'))}",
                    ]
                self.command(
                    "package-" + name,
                    [self.args.cargo, *arguments],
                    ROOT,
                    self.packaging_env,
                )
                version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"][
                    "package"
                ]["version"]
                original = self.target / "package" / f"{name}-{version}.crate"
            copied = self.output / original.name
            shutil.copyfile(original, copied)
            packages[name] = inspect_archive(copied, self.external / "packages", name)
            packages[name]["original_archive"] = str(original)
            vcs = Path(packages[name]["root"]) / ".cargo_vcs_info.json"
            packages[name]["cargo_vcs_info"] = (
                json.loads(vcs.read_text()) if vcs.exists() else None
            )
        self.summary["packages"] = packages
        self.summary["lanes"]["archives"] = "pass"
        runtime = packages["htn-planner"]
        frontend = packages["htn-planner-ron"]
        for kind in ("runtime", "frontend"):
            consumer = self.external / kind
            consumer.mkdir()
            dependencies = f'htn-planner = {{ path = {json.dumps(runtime["root"])}, version = "={runtime["version"]}" }}\n'
            if kind == "runtime":
                shutil.copyfile(
                    ROOT / "scripts/htn-package-consumers/runtime.rs",
                    consumer / "main.rs",
                )
                source = consumer / "main.rs"
            else:
                dependencies += f'htn-planner-ron = {{ path = {json.dumps(frontend["root"])}, version = "={frontend["version"]}" }}\n'
                source = Path(frontend["root"]) / "examples/warehouse.rs"
            manifest = f'[package]\nname="{kind}-consumer"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n{dependencies}\n[[bin]]\nname="{kind}-consumer"\npath={json.dumps(str(source))}\n[patch.crates-io]\nhtn-planner={{path={json.dumps(runtime["root"])}}}\n'
            (consumer / "Cargo.toml").write_text(manifest)
            self.lane = kind + "-native"
            self.cargo(kind + "-lock", ["generate-lockfile", "--offline"], consumer)
            self.cargo(kind + "-native", ["build", "--offline", "--locked"], consumer)
            binary = (
                self.target
                / "debug"
                / (kind + "-consumer" + (".exe" if os.name == "nt" else ""))
            )
            empty = self.external / "empty-cwd"
            empty.mkdir(exist_ok=True)
            stdout = self.command(
                kind + "-execute", [binary], empty, {"PATH": os.defpath}
            )
            self.summary[kind + "-native"] = {
                "binary_sha256": digest(binary),
                "artifact": self.retain(binary, kind + "-native"),
                "stdout": stdout,
            }
            self.summary["lanes"][self.lane] = "pass"
            self.lane = kind + "-wasm-compile"
            self.cargo(
                kind + "-wasm",
                [
                    "build",
                    "--offline",
                    "--locked",
                    "--target",
                    "wasm32-unknown-unknown",
                ],
                consumer,
            )
            wasm = (
                self.target / "wasm32-unknown-unknown/debug" / (kind + "-consumer.wasm")
            )
            if wasm.read_bytes()[:4] != b"\0asm":
                raise ValueError("consumer did not produce a WASM module")
            self.summary[kind + "-wasm"] = {
                "sha256": digest(wasm),
                "artifact": self.retain(wasm, kind + ".wasm"),
                "executed": False,
            }
            self.summary["lanes"][self.lane] = "pass"
            self.lane = "dependency-isolation"
            for target in (self.host, "wasm32-unknown-unknown"):
                suffix = "native" if target == self.host else target
                args = ["metadata", "--offline", "--locked", "--format-version", "1"]
                args += ["--filter-platform", target]
                metadata = json.loads(
                    self.cargo(kind + "-metadata-" + suffix, args, consumer)
                )
                validate_metadata(
                    metadata,
                    self.allowed_manifests(consumer),
                    consumer,
                    kind == "runtime",
                )
                self.cargo(
                    kind + "-tree-" + suffix,
                    [
                        "tree",
                        "--offline",
                        "--locked",
                        "--edges",
                        "normal,build",
                        "--target",
                        target,
                    ],
                    consumer,
                )
        self.summary["lanes"]["dependency-isolation"] = "pass"
        self.identity(runtime)
        self.lane = "immutable-packages"
        for package in packages.values():
            verify_unchanged(package)
        self.summary["lanes"][self.lane] = "pass"
        self.lane = "source-stability"
        if source_receipt() != before:
            raise ValueError("producing source changed during qualification")
        self.summary["lanes"][self.lane] = "pass"
        self.save()

    def allowed_manifests(self, consumer):
        return [
            consumer / "Cargo.toml",
            *(
                Path(package["root"]) / "Cargo.toml"
                for package in self.summary["packages"].values()
            ),
        ]

    def identity(self, runtime):
        self.lane = "identity-node-wasm"
        consumer = self.external / "identity"
        consumer.mkdir()
        shutil.copyfile(
            ROOT / "scripts/htn-package-consumers/runtime.rs", consumer / "fixture.rs"
        )
        (consumer / "lib.rs").write_text(
            '#[path="fixture.rs"]\nmod fixture;\n#[unsafe(no_mangle)]\npub extern "C" fn identity_byte(index:u32)->u32 { fixture::identity().map_or(256, |bytes| bytes.get(index as usize).map_or(256, |byte| u32::from(*byte))) }\n'
        )
        (consumer / "Cargo.toml").write_text(
            f'[package]\nname="identity-consumer"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\ncrate-type=["cdylib"]\n[dependencies]\nhtn-planner={{path={json.dumps(runtime["root"])},version="={runtime["version"]}"}}\n'
        )
        self.cargo("identity-lock", ["generate-lockfile", "--offline"], consumer)
        self.cargo(
            "identity-wasm",
            ["build", "--locked", "--offline", "--target", "wasm32-unknown-unknown"],
            consumer,
        )
        metadata = json.loads(
            self.cargo(
                "identity-metadata-wasm",
                [
                    "metadata",
                    "--offline",
                    "--locked",
                    "--format-version",
                    "1",
                    "--filter-platform",
                    "wasm32-unknown-unknown",
                ],
                consumer,
            )
        )
        validate_metadata(metadata, self.allowed_manifests(consumer), consumer, True)
        self.cargo(
            "identity-tree-wasm",
            [
                "tree",
                "--offline",
                "--locked",
                "--edges",
                "normal,build",
                "--target",
                "wasm32-unknown-unknown",
            ],
            consumer,
        )
        wasm = self.target / "wasm32-unknown-unknown/debug/identity_consumer.wasm"
        native = self.summary["runtime-native"]["stdout"].strip()
        self.command(
            "identity-compare",
            [
                self.args.node,
                ROOT / "scripts/htn-package-consumers/compare-identity.mjs",
                wasm,
                native,
            ],
            self.external,
        )
        self.summary["identity"] = {
            "native_bytes_hex": native,
            "wasm_sha256": digest(wasm),
            "artifact": self.retain(wasm, "identity.wasm"),
            "execution": "Node, all32bytes; no browser/full-warehouse execution",
        }
        self.summary["lanes"][self.lane] = "pass"

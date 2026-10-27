#!/usr/bin/env python3
"""Public CLI regression tests; synthetic archives/processes, never real Cargo."""

import io
import json
import shutil
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

CHECKER = Path(__file__).with_name("check-htn-packages.py")


class ArchiveCliTests(unittest.TestCase):
    # test-tag: compatibility
    def test_domain_include_must_be_present_and_is_bound_in_archive_receipt(self):
        for present in (True, False):
            with self.subTest(present=present), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                files = {"src/lib.rs": 'const SOURCE:&str=include_str!("fixture.domain");'}
                if present:
                    files["src/fixture.domain"] = "supplied domain source\n"
                archive = archive_at(root, files=files)
                result = subprocess.run([
                    sys.executable, "-B", str(CHECKER), "--inspect-archive", str(archive),
                    "--package", "htn-planner", "--output", str(root / "receipt"),
                ], capture_output=True, text=True, check=False)
                if present:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    receipt = json.loads((root / "receipt/archive.json").read_text())
                    member = "htn-planner-0.1.0/src/fixture.domain"
                    self.assertIn(member, receipt["members"])
                    self.assertEqual((Path(receipt["root"]) / "src/fixture.domain").read_text(),
                                     files["src/fixture.domain"])
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("missing or escaping", result.stderr)

    def test_valid_archive_materializes_normalized_manifest_outside_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "runtime.crate"
            files = {
                "Cargo.toml": '[package]\nname="htn-planner"\nversion="0.1.0"\n[lib]\npath="src/lib.rs"\n',
                "src/lib.rs": "// synthetic package\n",
            }
            with tarfile.open(archive, "w:gz") as stream:
                for name, content in files.items():
                    data = content.encode()
                    member = tarfile.TarInfo("htn-planner-0.1.0/" + name)
                    member.size = len(data)
                    stream.addfile(member, io.BytesIO(data))
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(CHECKER),
                    "--inspect-archive",
                    str(archive),
                    "--package",
                    "htn-planner",
                    "--output",
                    str(root / "receipt"),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = json.loads((root / "receipt" / "archive.json").read_text())
            self.assertEqual(receipt["name"], "htn-planner")
            self.assertEqual(
                (Path(receipt["root"]) / "src/lib.rs").read_text(), files["src/lib.rs"]
            )


def archive_at(root, name="htn-planner", files=None, extra=None):
    archive = root / (name + ".crate")
    content = {
        "Cargo.toml": f'[package]\nname="{name}"\nversion="0.1.0"\n[lib]\npath="src/lib.rs"\n',
        "src/lib.rs": "// synthetic\n",
    }
    if files:
        content.update(files)
    with tarfile.open(archive, "w:gz") as stream:
        for path, value in content.items():
            data = value.encode()
            member = tarfile.TarInfo(name + "-0.1.0/" + path)
            member.size = len(data)
            stream.addfile(member, io.BytesIO(data))
        if extra:
            member = tarfile.TarInfo(extra)
            member.size = 1
            stream.addfile(member, io.BytesIO(b"x"))
    return archive


class RejectionCliTests(unittest.TestCase):
    def test_archive_rejections_are_nonzero_and_do_not_escape_destination(self):
        cases = [
            ({}, "../escape"),
            (
                {"Cargo.toml": '[package]\nname="htn-planner"\nversion.workspace=true\n'},
                None,
            ),
            ({"src/lib.rs": 'const SOURCE:&str=include_str!("missing.ron");'}, None),
            (
                {
                    "Cargo.toml": '[package]\nname="htn-planner"\nversion="0.1.0"\n[dependencies.x]\npath="/checkout"\n'
                },
                None,
            ),
        ]
        for files, extra in cases:
            with (
                self.subTest(files=files, extra=extra),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                archive = archive_at(root, files=files, extra=extra)
                result = subprocess.run(
                    [
                        sys.executable,
                        "-B",
                        str(CHECKER),
                        "--inspect-archive",
                        str(archive),
                        "--package",
                        "htn-planner",
                        "--output",
                        str(root / "out"),
                    ],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((root / "escape").exists())

    def test_frontend_requires_versioned_runtime_and_packaged_example(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = archive_at(
                root, "htn-planner-ron", {"examples/warehouse.rs": "fn main() {}"}
            )
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(CHECKER),
                    "--inspect-archive",
                    str(archive),
                    "--package",
                    "htn-planner-ron",
                    "--output",
                    str(root / "out"),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("versioned", result.stderr)


class IsolationCliTests(unittest.TestCase):
    def test_external_consumers_and_cache_reject_foreign_checkout_or_cargo_config(self):
        for marker, option in (
            (".git", "--external-parent"),
            (".git", "--build-cache"),
            (".cargo/config.toml", "--external-parent"),
            (".cargo/config", "--external-parent"),
        ):
            with (
                self.subTest(marker=marker, option=option),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary)
                forbidden = root / "foreign"
                forbidden.mkdir()
                marker_path = forbidden / marker
                marker_path.parent.mkdir(exist_ok=True)
                marker_path.write_text("synthetic marker; contents must not be read")
                called = root / "cargo-called"
                cargo = root / "cargo"
                cargo.write_text(f"#!/bin/sh\ntouch '{called}'\nexit 19\n")
                cargo.chmod(0o755)
                destination = (
                    forbidden if option == "--external-parent" else forbidden / "cache"
                )
                result = subprocess.run(
                    [
                        sys.executable,
                        "-B",
                        str(CHECKER),
                        "--output",
                        str(root / "out"),
                        "--allow-dirty",
                        option,
                        str(destination),
                        "--cargo",
                        str(cargo),
                    ],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(called.exists(), result.stderr)
                self.assertIn("ancestor", result.stderr)


class PipelineCliTests(unittest.TestCase):
    def run_pipeline(self, root, failure, supplied=True):
        runtime = archive_at(root)
        frontend = archive_at(
            root,
            "htn-planner-ron",
            {
                "Cargo.toml": '[package]\nname="htn-planner-ron"\nversion="0.1.0"\n[dependencies.htn-planner]\nversion="=0.1.0"\n',
                "examples/warehouse.rs": "fn main() {}",
            },
        )
        fake = root / "cargo.py"
        shutil.copyfile(
            CHECKER.parent / "htn-package-check-fixtures/fake-cargo.py", fake
        )
        fake.chmod(0o755)
        fake.with_suffix(".json").write_text(
            json.dumps(
                {
                    "fail": failure,
                    "archives": {
                        "htn-planner": str(runtime),
                        "htn-planner-ron": str(frontend),
                    },
                }
            )
        )
        node = root / "node"
        node.write_text(
            '#!/bin/sh\nif [ "$1" = --version ]; then echo fake-Node; exit 0; fi\nexit '
            + ("31" if failure == "node" else "0")
            + "\n"
        )
        node.chmod(0o755)
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                str(CHECKER),
                *(["--archives", str(runtime), str(frontend)] if supplied else []),
                "--output",
                str(root / "out"),
                "--allow-dirty",
                "--cargo",
                str(fake),
                "--node",
                str(node),
                "--external-parent",
                str(root),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        return result, json.loads((root / "out/summary.json").read_text())

    def test_serial_external_process_pipeline_records_every_required_lane(self):
        with tempfile.TemporaryDirectory() as temporary:
            result, receipt = self.run_pipeline(Path(temporary), None)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(all(value == "pass" for value in receipt["lanes"].values()))
            execution = [
                command
                for command in receipt["commands"]
                if command["log"].endswith("runtime-execute.log")
            ][0]
            self.assertTrue(execution["cwd"].endswith("empty-cwd"))
            self.assertEqual(execution["overrides"], {})
            for lane in (
                "runtime-native",
                "frontend-native",
                "runtime-wasm",
                "frontend-wasm",
                "identity",
            ):
                self.assertTrue(Path(receipt[lane]["artifact"]).is_file())

    def test_package_phase_uses_only_named_offline_archives_and_propagates_failure(
        self,
    ):
        for failure in (None, "package-frontend"):
            with (
                self.subTest(failure=failure),
                tempfile.TemporaryDirectory() as temporary,
            ):
                result, receipt = self.run_pipeline(
                    Path(temporary), failure, supplied=False
                )
                if failure:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(receipt["lanes"]["archives"], "fail")
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                packages = [
                    command
                    for command in receipt["commands"]
                    if "package" in command["argv"]
                ]
                self.assertEqual(
                    [
                        command["argv"][command["argv"].index("-p") + 1]
                        for command in packages
                    ],
                    ["htn-planner", "htn-planner-ron"],
                )
                for command in packages:
                    self.assertEqual(Path(command["cwd"]), CHECKER.parent.parent)
                    for flag in ("--offline", "--no-verify", "--locked"):
                        self.assertIn(flag, command["argv"])
                    self.assertTrue(
                        command["overrides"]["CARGO_HOME"].endswith(
                            "packaging-cargo-home"
                        )
                    )
                patch = packages[-1]["argv"][packages[-1]["argv"].index("--config") + 1]
                self.assertIn(str(CHECKER.parent.parent / "crates/htn-planner"), patch)

    def test_child_native_wasm_node_or_dependency_failure_cannot_pass(self):
        for failure in (
            "native",
            "wasm",
            "node",
            "graph",
            "checkout",
            "registry",
            "parser",
            "external",
        ):
            with (
                self.subTest(failure=failure),
                tempfile.TemporaryDirectory() as temporary,
            ):
                result, receipt = self.run_pipeline(Path(temporary), failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("fail", receipt["lanes"].values())
                self.assertFalse(
                    all(value == "pass" for value in receipt["lanes"].values())
                )


if __name__ == "__main__":
    unittest.main()

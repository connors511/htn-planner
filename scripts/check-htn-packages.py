#!/usr/bin/env python3
"""Qualify real Cargo packages in external consumers; without publishing."""

import argparse
import json
from pathlib import Path
import sys
from htn_package_check.archives import inspect_archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inspect-archive", type=Path)
    parser.add_argument("--package", choices=("htn-planner", "htn-planner-ron"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--archives",
        nargs=2,
        type=Path,
        metavar=("RUNTIME", "FRONTEND"),
        help="requalify existing actual Cargo archives",
    )
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument(
        "--offline", action="store_true", help="offline is mandatory and always enabled"
    )
    parser.add_argument(
        "--cargo-mode",
        choices=("local",),
        default="local",
        help="requires caller authorization for local Cargo",
    )
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--node", default="node")
    parser.add_argument("--external-parent", type=Path)
    parser.add_argument(
        "--build-cache",
        type=Path,
        help="dedicated external cache; never share concurrent runs",
    )
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if args.inspect_archive:
        if not args.package:
            parser.error("--inspect-archive requires --package")
        receipt = inspect_archive(
            args.inspect_archive, args.output / "packages", args.package
        )
        (args.output / "archive.json").write_text(json.dumps(receipt, indent=2) + "\n")
    else:
        from htn_package_check.runner import Run

        Run(args).execute()
        print(
            f"HTN packages: required lanes passed; receipt {args.output / 'summary.json'}"
        )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        print(f"HTN package check failed: {error}", file=sys.stderr)
        sys.exit(1)

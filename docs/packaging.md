# Independent package qualification

The checker produces real `htn-planner` and `htn-planner-ron` Cargo archives and consumes their unchanged normalized manifests and sources in external workspaces. It never publishes crates. See the [frontend README](../crates/htn-planner-ron/README.md) for authoring.

```sh
python3 -B scripts/check-htn-packages.py --offline \
  --cargo-mode local --output target/htn-packages/qualified-run
```

Use a new output directory. `--allow-dirty` records development sources explicitly. `--archives RUNTIME.crate FRONTEND.crate` requalifies supplied archives. `--inspect-archive FILE --package htn-planner --output NEW_DIR` inspects one archive without compilation. No mode permits publication or modifications to extracted sources.

Requires the pinned Rust toolchain, Cargo, Node, the installed `wasm32-unknown-unknown` target, and prepopulated public registry caches. Offline mode is mandatory. Missing prerequisites fail their lane. Compiler wrappers and inherited Cargo configuration are excluded. Consumer roots reject Git-checkout ancestors and ancestor Cargo configuration; caches also reject Git-checkout ancestors.

The checker serializes Cargo commands with two jobs and dev/debug=0. `--external-parent` chooses the parent of its retained external workspace. `--build-cache EXTERNAL_DIR` reuses a dedicated cache; never share it between concurrent runs. The frontend's unpublished runtime dependency uses an explicit packaging-only path patch, followed by a consumer patch to the extracted runtime archive. Consumers permit only recorded extracted paths and public crates.io dependencies.

`summary.json` records source/HEAD/dirty identities, toolchain/platform, command argv/cwd/status/logs, archive/member hashes, consumer locks/graphs, and binary/module hashes. Required lanes cover archive validation, runtime and frontend native execution, full consumer WASM compilation, Node identity execution, dependency isolation, unchanged extraction, and source stability. Not-run is never pass. Keep generated evidence under ignored `target/`; review the receipts before removing the separately retained external workspaces.

Native fixtures run from an empty directory. The warehouse fixture parses packaged `.domain` text and checks its separate RON compatibility journey. Both complete consumers compile for WASM, but only the runtime 32-byte identity shim executes in Node. This does not establish full frontend WASM or browser execution, host execution policy, or performance.

Lightweight process/archive tests use fake Cargo and Node processes:

```sh
python3 -B scripts/test-check-htn-packages.py
```

These tests qualify checker failure handling. Real package qualification remains a separate explicit check against the final source revision.

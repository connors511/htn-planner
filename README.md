# HTN planner

A generic, deterministic hierarchical task network planner in Rust.

- [`htn-planner`](crates/htn-planner/README.md) provides typed planning, explicit work and capacity limits, backtracking, and deferred tasks.
- [`htn-planner-ron`](crates/htn-planner-ron/README.md) compiles caller-supplied `.domain` and RON text into admitted planner definitions.

The planner has no engine, filesystem, network, clock, or random generator dependency. Applications supply immutable facts and execute the returned actions themselves.

## Use from Git

Use both crates directly from Git:

```toml
[dependencies]
htn-planner = { git = "https://github.com/connors511/htn-planner", rev = "<full commit SHA>", default-features = false }
htn-planner-ron = { git = "https://github.com/connors511/htn-planner", rev = "<same full commit SHA>", default-features = false }
```

Pin both crates to the same commit. See their READMEs for API and compatibility contracts.

## Development

Requires the toolchain in `rust-toolchain.toml` and cargo-nextest.

```sh
cargo fmt --all --check
cargo nextest run --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --lib --target wasm32-unknown-unknown --locked
cargo run -p htn-planner-ron --example warehouse --locked
python3 -B scripts/test-check-htn-packages.py
```

[Package qualification](docs/packaging.md) checks real Cargo archives in isolated native and WASM consumers and runs the identity fixture in Node. It requires prepopulated public registry caches. Generated evidence stays under ignored `target/` directories.

## License

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

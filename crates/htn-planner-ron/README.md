# htn-planner-ron

The `.domain` frontend is `domain::compile(root_id, sources, schema,
entry_signatures, limits)`, with each source supplied as `domain::Source { id,
text }`. It performs cold parsing/type inference and admits the existing typed
runtime IR. Includes resolve only supplied sources; no filesystem, network or
application callback is required. Host entry signatures type inputs whose use
does not otherwise determine a type, including unused inputs.

The deterministic supported profile has input-only methods, fresh-output axioms,
ordered `and`/enumerating `alt`, grounded `not`, facts/`#` axioms, `!` actions,
task/`&` deferred calls, integer/symbol literals and named constants. It rejects
`io`, `or`, floating-point/string values, arbitrary calls, planning-effect forms,
recursion, overloads and inheritance. It is inspired by the pinned urosidoki
[EnemyBase guards](https://github.com/urosidoki/htn_planner/blob/e1b39033f4824f4afc4f1462556856524ee73eab/Domains/Includes/EnemyBase.domain#L6-L20),
without a full-language compatibility claim. See the [authoring guide](https://github.com/connors511/htn-planner/blob/main/docs/authoring.md)
for the supported language profile. The runtime's enumerating Or differs
from committed FirstSuccess; unsupported source `or` aliases neither.

For a downstream adapter error on the lowered definition,
`domain::locate_error(root_id, sources, error)` resolves a known task/branch/step
locator against the same supplied source graph. It adds the originating file and
line/column to the error context; step zero identifies a method condition and
subsequent steps identify calls. Missing tasks/branches, unmapped steps or invalid
source graphs preserve the complete original error. This bounded cold lookup runs
only after failure. It performs no IO and installs no runtime source map or new
identity data. Application-specific action constraints and live boundary explanations
remain the downstream adapter's responsibility.

## Explicit RON compatibility

`compile(source_id, text, schema, limits)` parses versioned RON and calls the
runtime's admission boundary. It performs no IO. Parse errors retain source and
line/column; admission errors retain source, declaration and step. Unknown
vocabulary, references, arity/type/parameter errors, task recursion, invalid
entries and invalid limits reject before execution. There is no scripting,
external code loading or native code generator.

Version 2 declares names and resolves `Var` within task parameters and each
method's own locals (derived queries have their own parameter/local scope):

```ron
(version: 2, name: "named", entries: ["select"], tasks: [
  (name: "select", parameters: [Ref("actor", "Actor")], methods: [
    (name: "choose", locals: [Int("distance")], steps: [
      Query(name: "distance", arguments: [Var("actor"), Var("distance")]),
      Action(name: "move", arguments: [Var("actor"), Var("distance")])])])])
```

The host must register that example's Actor type, distance predicate and move
action. Binding declarations are `Ref("name", "Type")`, `Record("name", "Type")`,
`Int("name")`, `Symbol("name")`, `Bool("name")` and `Fixed("name", scale)`.
`Literal(...)` retains the runtime's existing typed Value syntax. Output
signatures remain ordered runtime types, not named slots. Names are ASCII
identifiers of 1–128 bytes; duplicates and parameter/local shadowing reject.
Unknown variables report source, task/query, method and step where applicable.
All nested conditions, child outputs and deferred arguments use the same names.

`authoring::Definition` and its public constituent types support cold typed
composition; `definition.lower(source_id)` returns the existing numeric
version-1 `htn_planner::Definition`. This is name resolution, not executable
admission: use `CompiledDomain::admit` on the result. Type/definite-binding
validation remains in runtime admission. Equivalent lowered definitions retain
identical schema/definition identity and decision charges; names are not runtime
metadata. Compile routes strictly on the explicit version (a cold header parse,
then a full parse); malformed version 2 never falls back to version 1. Owned
lowering bounds total cold authoring size and nested literal/condition depth
before copying, and disposes rejected owned trees iteratively. Caller-driven
cloning or dropping unsubmitted public authoring data remains caller-owned.

Run the synthetic warehouse example with:

```sh
cargo run -p htn-planner-ron --example warehouse
```

The executable first parses packaged `warehouse.domain` and plans a typed
candidate-to-action journey, including an unused host-typed entry input. Its
unchanged RON compatibility journey registers typed Parcel/Worker references and
Role/Bay records, derives eligible workers, and allocates three distinct roles.
The first front worker prevents the right assignment; complete-continuation
backtracking selects the alternative. Assert/Retract planning effects (the
`claim`/`release` registrations) remain hypothetical: caller facts and real
reservations stay unchanged. A deferred delivery chooses a bay from a fresh
snapshot while preserving its captured parcel. Changed definition or limit
identity rejects the saved token. Small independent cases check empty success,
NoPlan, work/capacity/input failures, numeric overflow, exact host schema matching,
and Or versus FirstSuccess child commitment (including further selected-child
witnesses). The executable needs only these two packages and supplied `.domain`/RON text;
no application initialization, content directory, clock or service is used.

The host supplies an admitted immutable snapshot and rechecks actual execution
legality. Optional deterministic `QueryProvider` batches use declared bound input
prefixes, typed finite rows and the shared meter; they do not authorize arbitrary
expression callbacks, host-state mutation, clocks or RNG. Candidate validation,
cache/storage and query row/probe/effect work share the decision allowance.
Host snapshot construction costs remain separate. See `htn-planner`'s bundled README for public construction,
numeric, limits, locators, compatibility and inert-result contracts.

The standalone lifecycle example reports host execution and fresh deferred work:

```sh
cargo run -p htn-planner-ron --example lifecycle
```

Its warehouse host owns accepted root intentions separately from the expanded
execution suffix/cursor, an active action's progress, a per-agent logical-tick
attempt guard, and the current observation. It reports start/progress/completion,
rejected starts, failed execution, empty completion and distinct planner failures.
Actions are revalidated against actual object availability and positions at start;
planning never changes those observations. Deferred expansion occurs only while
idle using fresh facts. A failed expansion preserves the pending token and adopts
no tentative prefix. At most one initial/deferred planning attempt runs per
logical tick, including failed or empty attempts; backwards/repeated ticks are
suppressed. Progress also advances at most once per supplied logical tick.

The RON checkpoint includes accepted root intentions, execution entries/cursor,
active action/progress and last progress tick, the last planning tick, captured
deferred inputs, and definition/semantics identities. Resume rejects incompatible
identities and invalid cursor/progress state. Equivalent root intentions preserve
both active progress and completed prefixes, including after source relocation
or deferred suffix replacement. A completed equivalent routine remains completed;
call reset to request a deliberately repeated lifecycle. Reset clears accepted
work/progress while retaining the same-tick scheduling guard, so a new attempt
waits until a later tick. This small host is example policy, not a general
executor or a production scheduling/performance claim. Snapshot construction and
host execution costs are separate from the planner's reported work.

Run the reusable pack fixture with:

```sh
cargo run -p htn-planner-ron --example pack
cargo run -p htn-planner-ron --example commitment
```

Derived queries enumerate eligible actors and test grounded absence through
private existential scopes. Registered planning effects claim hypothetical roles.
The first front actor makes the right role impossible, so a later continuation
reopens front and produces the first complete unique mapping. Actual starting
reservations and facts stay unchanged. The executable fixture is shared with a
public RON/compiler/runtime test. `Condition::Or` reopens later children on caller
failure; `FirstSuccess` instead commits its selected child while retaining that
child's own witnesses. Runtime tests contrast those behaviors explicitly.

Optional `queries` and Rust-registered schema `effects` default empty. The
recommended RON compatibility version is 2; legacy numeric version 1 remains accepted.
Planner compatibility remains
`htn-planner/schema-2/semantics-2`. There is no arbitrary host callback or script
extension in this vocabulary. All derived/probe/effect operations use the same
metered decision; exhausted negation is an error, never proof of absence.


Package consumption is checked from real `.crate` archives by the repository's
`scripts/check-htn-packages.py`; the standalone warehouse Rust/`.domain`/RON and helper
files are included in this package. The checker uses explicit external paths and
an unpublished-runtime patch, never a private registry. Native execution, full
consumer WASM compilation, and Node execution of the 32-byte identity fixture
are separate lanes. A WASM build does not establish browser execution. No crates
are published by this workflow.

At the RON `compile` boundary, owned schemas are validated before source/text
checks and parsing. Invalid schemas
receive a bounded `<schema>` locator; oversized source identities use the fixed
placeholder. Rejected schema literal values are disposed iteratively, including
when text is malformed or over capacity. This cleanup is linear in caller-transferred
input and outside planner decision metering. A valid mutable authoring schema is
still independently checked by `CompiledDomain::admit`; schema validation alone
grants no execution capability.
This guarantee covers the RON frontend's owned boundary, not earlier host-wrapper
rejections or arbitrary raw caller-owned tree destruction/cloning.

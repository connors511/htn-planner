# htn-planner

A pure interpreter for validated hierarchical task definitions. Applications
register a `Schema`, construct an immutable `FactSnapshot`, admit a `Definition`
with explicit `Limits`, and call `plan` with borrowed entry arguments. Rejected input remains caller-owned. `htn-planner-ron` compiles caller-supplied
RON into the same admission boundary. The runtime uses Serde and BLAKE3;
it has no parser, application-specific types, filesystem, network, clock or random generator.

`CompiledDomain` is immutable and cannot be deserialized directly. Every decision
owns isolated temporary search state. Schema and facts must match exactly.
`Outcome::Success` contains a complete vector of inert `PlanEntry` values:
`Action(Action)` or `Deferred(DeferredTask)`. Empty
success is meaningful. Other outcomes never expose a tentative prefix:
`NoPlan`, `BudgetExhausted`, `CapacityExceeded`, and `InvalidInput`. A bounded
single diagnostic identifies source, task, method and step (or entry for invalid
invocation input); executable entries identify their source task and method with
concrete bound arguments. Locator.task defaults to None when older diagnostic
metadata has no task, and parse/schema failures may have no task. Applications execute actions and recheck execution legality themselves.

Methods and steps follow vector order. Facts retain caller-supplied row order;
callers must supply a deterministic order with stable ties. The interpreter
performs depth-first search over complete continuations, retaining earlier method
choices until the entire remaining sequence succeeds. Failure restores the
previous continuation and tentative plan. No action changes facts. An optional pure query provider can enumerate demanded
rows during search; it cannot execute application actions.

Task `parameters` are bound inputs; optional `outputs` declare return types.
Each method declares invocation-local `locals` (initially unbound) and a `result`
expression for every task output. `Parameter(index)` reads the immutable task
input; `Local(index)` selects a method-local slot. `Require`, `Action` and task
inputs must read bound values. Admission follows definite binding through each
method and rejects invalid slots, types, output arity and unbound reads.

`Query(name, arguments)` enumerates admitted fact rows in order. A previously
unbound local is bound; an existing local, parameter or literal constrains the
row. Repeating a local in one tuple requires equal values. Later queries cannot
overwrite bindings. `Call(name, arguments, outputs)` passes bound inputs into a
fresh invocation and explicitly unifies successful results into caller output
expressions. A conflict is a failed branch, not invalid runtime input. The
zero-output `Task` form rejects callees that declare outputs. Entry task outputs
are evaluated and discarded; only complete inert action vectors are returned.

Query cursors and method alternatives retain their entire pending continuation,
local environments, speculative overlay and tentative action prefix. A later sibling may reopen an
already successful child method or query row. Child output unification is rolled
back with that continuation; each new method/call gets independent local slots.
The first complete plan wins; this is not a cost optimizer.

Optional `Schema.query_inputs` registers each lazy predicate's bound input-prefix
length. Admission rejects an unbound prefix in Query, Require or Fact conditions.
The default empty registration preserves existing static-only schema identities.
`plan_with_queries(domain, facts, provider, entry, arguments)` and
`expand_deferred_with_queries(domain, facts, provider, token)` borrow a
`QueryProvider`; traced variants add the same independent capacity argument as
the static APIs. The provider observes one immutable world for the complete
attempt and returns deterministic, typed, complete tuples preserving that prefix.
It may not perform IO, use clocks/RNG, mutate host state or reenter planning.

The provider must precharge scans/copies and reserve all simultaneous returned
rows and private scratch through `QueryBudget` before work/allocation. Its failure
is sticky and aborts the attempt as BudgetExhausted, CapacityExceeded or InvalidInput,
including inside absence probes; exhaustion is never ordinary missing facts.
This is a cooperative pure-host contract, not enforcement against arbitrary
third-party callbacks. Returned rows are validated before becoming visible.
A predicate/prefix batch is materialized once per attempt and survives speculative
branch rollback. Queries see original static rows, that cached batch, then visible
overlay insertions; retractions also hide matching lazy rows. The cache remains
inside the attempt's work/storage accounting and never crosses fresh expansion.

`CompiledDomain::snapshot(rows)` shares its already-admitted immutable schema and
validates only the supplied rows. It avoids repeated full schema admission/deep
cloning for live snapshots. The standalone `FactSnapshot::new(schema, rows)` still
validates an untrusted schema. Pointer-identical admitted schemas use a fast match;
independent equivalent schemas retain structural compatibility checks.

Derived relationships are declared in optional `Definition.queries` (default
empty). Each `DerivedQuery` declares bound `parameters`, typed `outputs`, private
`locals`, one `condition`, and grounded `result` expressions. A
`Condition::Derived` supplies bound inputs and explicit output unification sites.
Every invocation owns independent locals; a later caller failure can reopen its
remaining witnesses. `Step::Condition` evaluates the same condition vocabulary
inside a method. Primitive `Step::Query` remains a fact-condition shorthand.

`Condition::And` evaluates children in order and retains alternatives across the
complete caller continuation. `Or` enumerates every child's witnesses in order.
`FirstSuccess` selects the first child that yields a witness: later failure may
reopen witnesses inside that child, but never another child. Earlier outer
choices remain available and can create a fresh invocation/selection. For
example, children producing only actor A then only B with a later requirement
for B succeed under `Or` and fail under `FirstSuccess`. If the first child itself
produces A then B, both forms can choose B. Empty And succeeds once; empty Or and
FirstSuccess have no solution. Definite binding after a disjunction is the
intersection of its children's bindings.

`Not` requires every caller-scope expression to be grounded and exports no
bindings. A zero-output derived query inside Not may use its own private
existential locals. Not restores its probe state and succeeds once only after
complete absence; BudgetExhausted, CapacityExceeded and InvalidInput propagate
through it. Probes, derived calls and commitment use the same explicit interpreter
stack and decision allowance. Admission rejects condition depth overflow,
unsafe negation, unbound results and cycles through any derived condition edge.

Optional `Schema.effects` registers finite typed `PlanningEffect` programs.
`Step::Effect` supplies bound arguments to ordered `Assert`/`Retract` operations;
operation expressions admit only Literal and Parameter. Effects have no callbacks,
hidden searches, IO, randomness or live reservations. Each branch owns a
private speculative overlay. Assert of a visible exact tuple is idempotent;
Retract hides every visible exact match. Reassert appends a new insertion.
Queries see visible original rows followed by visible insertions in operation
order. Rollback restores overlay, bindings, continuation and tentative actions
together. Effects emit no executable entry and never change the input snapshot;
applications recheck real observations when executing accepted actions.

`Defer(name, arguments)` emits an inert future invocation with fully bound inputs;
its target must declare no outputs. Deferred references participate in the same
finite acyclic task admission. `expand_deferred(domain, fresh_facts, token)` may
start a private task only if an admitted Defer declared it as a target. It checks
the captured definition identity, planner semantics, schema, target and input
values through one metered decision; it never resets allowance for an inner
planner. Cached definition identities avoid hashing during decisions. A new
snapshot can change local choices, while captured input values keep their exact
typed identity. Missing facts can produce NoPlan; malformed typed input or an
incompatible token produces InvalidInput with a bounded reason. Failed expansion
never returns a usable prefix. Successful empty expansion is completion.

Deferred tokens, plan entries, actions and locators support Serde for host-owned
persistence. Deserialized tokens remain untrusted until expansion; persisted
Action requests still require the host's normal execution validation. The
runtime supplies no clock, scheduler or universal executor. Hosts own accepted
intentions, cursor, actual action progress, invalidation and reset, and decide
when a safe boundary permits fresh expansion. Include definition/semantics
identities, captured inputs and scheduling state in checkpoints. Read-only
schema()/definition() expose admitted source data for content export; they do
not grant mutable access to executable definitions. `validate_task_inputs` checks
concrete arguments for declared public/private tasks through the bounded schema
validator without running search or consuming a decision allowance. This grants
no execution capability; a valid invocation may still exhaust planning limits.

`Type::Fixed(scale)` preserves scale as schema identity; scales 0 through 18 are
accepted. `Int` stores signed i64 values. Compare supports exact signed Equal, NotEqual,
Less, LessEqual, Greater and GreaterEqual. Calculate supports checked Add and
Subtract, with ordinary output unification. Both operands must be Int or the
same Fixed scale; results preserve that type/scale. Overflow is fatal InvalidInput
with a bounded arithmetic-overflow reason, including inside Not/Or. A grounded
output mismatch is an ordinary failed branch. No implicit conversion occurs. References preserve a registered host type and u64 key. Records preserve
a registered type and validate fields recursively through an explicit bounded
work stack. Symbols are exact UTF-8 strings. Nested values are limited to 128
levels, one million nodes/bytes and 1024 pending validation entries.

Owned admission installs private cleanup owners before rejection: `FactSnapshot::new`,
`CompiledDomain::admit`, and consuming `Schema::validate` dispose rejected nested
literal values iteratively. Cleanup is linear in the owned input, outside decision
metering; this is not a constant-time or allocator-byte bound. Successful admission
retains the original inputs unchanged. `Schema::validate` returns raw authoring data,
not an execution capability or permanent trust after mutation. Raw caller-owned
`Value`/`Expression`/`Definition` trees remain the caller's responsibility outside
these reusable runtime/frontend owned boundaries; arbitrary-depth clone/drop is
not made universally safe. A host wrapper rejecting raw input before core admission
must manage that ownership separately.

Limits are positive: work <= 1,000,000; storage <= 1,000,000; combined task/condition/scope depth <=
128; plan length <= 1024; live argument bindings across all retained branches <= 1024; diagnostic
capacity <= 4096. `Limits::small()` is an example profile, not a production
performance claim. Optional `plan_traced` and `expand_deferred_traced` capture
bounded diagnostic nodes independently of search work/storage. Requested capacity
zero retains metadata only; truncation never changes the result or authoritative
work report. See [TRACE.md](TRACE.md) for logical-unit retention accounting.

Work precharges cached schema comparison, entry-name comparisons, each caller
value node/string byte and registered-type lookup, interpreter item, expression,
fact row, value equality, clone, plan-entry append and choice restoration.
Condition frames, private scopes, numeric operations, branch pruning/probe
cleanup, overlay visibility scans, tuple equality and effect mutations share
that allowance. Choice copies include the overlay payload and tombstones.
Deferred identity/semantics/target checks consume that same allowance; deferred
entry storage counts identity bytes and task/semantics/provenance strings. Immutable
literal/fact costs are cached during admission. Each pending item and branch
stores an incremental storage cost; decision accounting never rescans branch
payloads to discover their size. Work exhaustion may precede input rejection.

Storage uses deterministic logical units: one per branch/frame/action/value node,
one per declared input/local slot and cached cost slot, plus each bound value
node and retained string UTF-8 byte. Unbound locals count toward the global
binding/storage capacity before initialization. It includes
all retained choice continuations, bindings, tentative actions and temporary
expression copies before allocation. Task arguments move into the current
method; choice capture needs only two simultaneous branch copies. Immutable
schema/definition and caller-owned facts/entry inputs are outside search storage;
accepted argument copies and validation scratch are bounded. Vector allocator
spare capacity and Rust object layout are not byte quotas. Capacity checks are
conservative; no candidate truncation means `NoPlan`. Diagnostics retain at most
one bounded source/entry/reason record independently of search storage.

Admission caps schema/definition/snapshot aggregate logical size at 1,000,000;
schema declaration count at 4096 (plus at most 4096 registered effects), task/entry/derived-query count at 1024, methods per task at
1024, steps per method at 16384, arity/record width at 1024, condition fanout at 65536, effect operations per registration at 16384, and every declaration
name/source identifier at 1024 bytes. Validation uses an explicit stack with at
most 1024 entries and depth 128. Source text is limited to 1,000,000 bytes before
parsing. Admission and snapshot construction are separate from decision work.
Performance suitability remains workload-specific; application qualification does not
establish a general library latency guarantee.

The compatibility identity `htn-planner/schema-2/semantics-2` is independent of all
host persistence formats. Authoring Definition.version remains 1 with optional queries/effects defaulting empty. Re-admitting older content creates the new identity; saved tokens from older semantics must reject explicitly. Both packages currently version together at 0.1.0; consumers
must match their definition schema/semantic identity. Serialization of the
untrusted definition does not substitute for admission.

The library uses portable Rust and is intended for native and
`wasm32-unknown-unknown` consumers. Native/WASM and packaged external-consumer
qualification are exercised separately from application integration.
No registry publication is performed.

## Definition identity

`CompiledDomain::identity()` returns a cached `DefinitionIdentity`.
The opaque identity supports equality, Serde persistence and `as_bytes()` for its
32-byte BLAKE3 value. It hashes the admitted schema, definition and every limit,
plus `COMPATIBILITY_ID`. Diagnostic source locations are excluded; semantic
schema, domain, task, method and action names remain included. Moving a source
file preserves identity; changing a concrete action, schema or limit changes it.
A saved plan or deferred continuation must compare its original identity against
the currently admitted domain before reuse. Identity alone is not admission.

The private streaming codec has prefix
`HTN-PLANNER-DEFINITION-IDENTITY\0CODEC-1\0`. It serializes the ordered tuple
`(COMPATIBILITY_ID, schema, definition, limits)` using derived Serde fields,
including field names and type/variant names, so additive semantic fields are
included automatically. Primitive type, collection, field, element and end tags
separate values; strings/bytes/collections have explicit lengths. Integer values
use fixed-width little-endian bytes, enum indices use u32, and lengths use checked
u64. Serde encodes usize/isize as u64/i64 on both native and WASM, independent of
pointer width. Floating point, maps, unknown collection lengths and display-based
serialization fail explicitly; the admitted vocabulary uses none of them.

This identity codec is compatibility-sensitive. Changing serialization shape,
tags, framing or the planner compatibility identity can invalidate saved identities;
such changes require an explicit compatibility decision. It is not a general
purpose serialized-definition format, and no parser is involved. Admission computes
the identity once and rejects unsupported representations. Reading the cached
identity does not hash definitions or consume planning work.

`Definition`, `PlanEntry` and `CompiledDomain` expose `heap_footprint()` for host
history accounting. `HeapFootprint.capacity_bytes` counts owned String/Vec
capacities and Box pointees; `clone_bytes` counts their current lengths and Box
pointees under the current Clone implementations. Compiled domains include their
raw schema/definition/source and private compiled programs. Recursive authoring
values/conditions are traversed iteratively, including unadmitted inputs. Inline
self, allocator headers and traversal scratch are excluded. These target-specific
structural bytes are separate from deterministic search storage units and are
not measured RSS, allocation count or peak memory.


## Standalone package checks

The repository's `scripts/check-htn-packages.py` packages this runtime and its RON
frontend, extracts the real archives without modification, and builds separate
external workspaces. The programmatic runtime caller registers typed references
and records and depends on this runtime alone; its normal/build graph excludes
the RON frontend and data parsers. Serde derive compiler support is permitted.
The full warehouse consumer exercises the packaged frontend example. Both native
executables run from an empty directory with a minimal environment; the same
sources compile for `wasm32-unknown-unknown`. A separate Node WASM identity caller
compares all32bytes against native. These are qualified separately and make no
browser-execution or performance claim. Planning remains filesystem/service-free;
Cargo and cached public dependencies are build prerequisites only. Independent
planner identities never depend on application content or persistence identities.

## Lazy immutable query batches

`CompiledDomain::snapshot(rows)` validates new rows against its shared immutable
admitted schema. It avoids rebuilding/validating/cloning that schema per snapshot.
The raw `FactSnapshot::new(&schema, rows)` convenience path stays fully checked;
semantically equivalent independent schemas remain compatible. Sharing is an
optimization, never a process-address compatibility contract. The domain keeps a
cold authoring schema for its existing constant accessor and a shared schema for
snapshots. Empty lazy-query metadata preserves the existing definition identity
and static-path logical charges.

Declare a predicate's fixed required input prefix with `Schema::query_inputs`
(`QueryInputs { predicate, bound }`). Admission rejects duplicate/unknown entries,
invalid prefix lengths, and a Query/Fact condition whose prefix can be unbound.
Bound zero is an explicit whole-predicate batch. The ordinary static APIs remain
usable without a provider.

`plan_with_queries`, `plan_traced_with_queries`,
`expand_deferred_with_queries` and `expand_deferred_traced_with_queries` borrow a
`QueryProvider` for one synchronous attempt. The provider sees only a declared
predicate and its concrete input prefix. It returns complete typed rows in stable
order, including those exact inputs. It must be pure over an immutable world:
no RNG, clock, I/O, host-state mutation or reentrant planning. Core validates every
returned row and memoizes each predicate/prefix once across backtracking. A fresh
deferred attempt uses a fresh provider/cache; no world reference is stored in a
domain, token or accepted plan.

Providers must charge `QueryBudget` before work and reserve their complete live
logical payload/scratch before allocating. Reservations are peak declarations,
not incremental additions; include a row unit, two slots per argument, recursive
Value nodes and UTF-8 bytes. Failures are sticky even when a provider ignores an
error. Core additionally validates, accounts cached key/rows and validation
scratch, and includes the shared cache in every active/retained branch storage
check. These units are deterministic conservative accounting, not allocator bytes.
Arbitrary third-party code cannot be made pure or bounded by this cooperative
interface alone. Budget/capacity/invalid-provider failures abort the entire
attempt with no executable prefix; empty rows mean ordinary absence.

For a query, rows appear in static snapshot order, then the demanded prefix's
provider order, then branch-local effect insertion order. Effects can hide either
static or provider rows and rollback restores that visibility without rerunning
the provider. Different prefix batches never change a prefix's compatible-row
ordering. Provider batches are demand driven even with tracing enabled.
`QueryCandidate` trace events expose validated typed rows once at materialization;
`Witness` denotes an actual matched row. Rejection events describe inspected
incompatible rows. Detailed retention remains independently capped before copying
and does not change logical work, search outcomes, or provider calls.

Optional traces retain `work_failure`, the first failed work charge, as fixed-size
metadata independent of the node payload cap (including cap zero). Its indices
resolve with bounds checks against the exact compiled domain. The search cursor
can denote method end or previous context during cleanup. Its provider predicate is present
only during a host query. Requested work is recorded before execution, not a
measurement of actual CPU cost. No arguments or extra world queries are retained.

The fixed `provider_work` aggregate counts actual host calls, their consumed work,
and the first largest call. It excludes cache/key handling, returned-row validation
and search. A failed charge contributes only the allowance remaining at failure;
`work_failure.requested` records the full request. These units do not measure CPU.

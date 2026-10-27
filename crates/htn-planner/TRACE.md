# Bounded planning inspection

`plan_traced(domain, facts, entry, arguments, requested_capacity)` and
`expand_deferred_traced(domain, fresh_facts, token, requested_capacity)` return
`TracedPlanningResult { result, trace }`. The ordinary `plan` and
`expand_deferred` APIs retain no trace. All variants run the same interpreter and
Meter. The complete PlanningResult, work/storage usage, stage counters, candidate
order and returned inert plan are identical for every capture setting.

A trace owns append-only structured nodes for admitted tasks, methods, fact and
derived queries, condition/control boundaries, alternatives, steps, effects,
witnesses, rejections, restorations and final result. Locators include source,
task, declaration and source step. Declaration kind is explicit in the event;
query/effect names are distinct from task-qualified method names. Bound arguments
are typed Value snapshots; None marks an unbound local. Invalid/unvalidated
inputs are omitted with a fixed marker. Numeric Compare snapshots contain
left/right operands; Calculate snapshots contain left/right/output constraint,
with the output populated in a successful local Witness. Tracing never sizes,
formats or copies
unvalidated caller values or a deserialized token's locator. Existing admission
and invocation guards run once; capture reuses validated cached value costs.

Node IDs are local to one returned trace. Every nonroot parent refers to an
earlier retained node. Invocation context travels with existing bounded search
frames/choice records, so backtracking preserves the correct ancestry. IDs never
control search or host identity. FirstSuccess selects one child while its
remaining witnesses stay available; earlier outer alternatives remain available.
A negation probe records witness rejection or restored absence. Local witnesses
and StepCompleted events can be followed by caller failure. An Action step
constructs an inert request; it does not execute it. The authoritative final
accepted plan comes from `result`, never from whichever nodes fit.

Lazy-provider batches emit `QueryCandidate` once per validated row when the
predicate/prefix is actually demanded. The node carries the predicate locator
and complete typed tuple, beneath the requesting query/effect node. It is a
candidate, not a successful witness. Inspected incompatible provider rows emit
`Rejected(UnificationConflict)`; only actual matches emit `Witness`. A cached
prefix is never rematerialized merely to explain another branch, and tracing
never demands an otherwise unused provider batch. Candidate payloads use the
same cap-before-copy checks; malformed unvalidated rows are never copied.
`QueryCandidate` is appended to the optional event enum, preserving existing
variant order for static traces. Native/WASM APIs use the same representation.

Deferred expansion starts a fresh trace universe. Its root identifies a deferred
attempt and its task/source context comes from the currently admitted target.
Captured token diagnostic strings are untrusted and are not copied into nodes.
The host retains and joins its own accepted-decision/token provenance; an ID from
an earlier trace is never a parent in the new trace.

## Retention units

Effective capacity is min(requested_capacity, domain.limits().diagnostics), whose
admitted maximum is 4096. Nothing silently raises the default small profile's
cap 16; a long source/declaration can legitimately prevent even its root fitting.
The returned trace records the actual effective cap. Capacity zero retains no
node-vector allocation. Fixed metadata remains available: admitted domain
identity, capacity/used units, truncation/dropped flags, final outcome and complete
final work report. This fixed metadata is outside the variable retention cap.

Each retained node costs one fixed-header unit plus source/declaration/task UTF-8
bytes (and one unit for a present task), plus one argument-slot unit and the
cached node/string-byte cost of each present Value. The fixed-header unit covers
the node's fixed fields, IDs, enums, work counters and owned-container headers.
This is a deterministic logical quota, not allocator bytes. String/Vec spare
capacity, allocator metadata and fixed struct layout are not byte quotas. The
node-count bound and dynamic-payload bound both follow from the 4096-unit maximum.
Fixed per-frame diagnostic IDs/flags are part of the existing search frame
header overhead; they add no uncapped ancestry buffer and do not change Meter
charges/reservations according to trace settings.

Preflight occurs before constructing owned locators, snapshot vectors or cloned
nested Values. On the first retention failure capture stops permanently for that
decision. Later hooks update only a bounded scalar dropped-emission count; they
do not walk or build discarded payloads. There is no eviction, recycled ID,
dangling parent or growing history. dropped_emissions counts skipped hook
emissions, including the first denied emission, rather than hypothetical unseen
search-tree cardinality. It saturates at u64::MAX; dropped_saturated means a
further increment could not be represented. Truncation changes diagnostics only.

Trace/TraceNode/enums and WorkReport/StageCounters support Serde for inspection
DTOs and external harnesses. They are diagnostic data, grant no execution or
admission capability, and introduce no authoritative persistence/decoder protocol.
Hosts decide whether and how to retain one bounded last trace, copy it for
read-only inspection, and clear it on reset/death/terminal transitions. Each API
call owns fresh search/capture storage; a prior failure or retained trace cannot
change the next call. Total retained memory still scales with host actor count.

## Always-on stage counters

WorkReport.stages uses the always-on `htn-planner/stages-1` instrumentation profile,
including when capture is disabled. Every u64 counter saturates; saturated means
at least one increment overflowed. Current positive work maxima already bound
reachable counts far below u64::MAX. Trace-only dropped/truncated metadata is
separate from this shared report. No clock, RNG or high-cardinality metric label
is involved.

- tasks_entered: a validated invocation enters the precharged search loop once,
  before selecting methods; a task with no methods still counts.
- methods_entered: after the method's local initialization precharge succeeds.
- queries_entered: fact, derived or grounded Require invocation at successful
  precharged dispatch/argument preparation;
  resumed rows inside that invocation do not increment it.
- conditions_entered: logical condition-node entry at precharged dispatch,
  including fact/derived nodes; resumed row cursors are not new entries.
- rows_inspected: completed candidate inspection in fact enumeration, Require,
  or effect existence/retraction scans. Hidden/tombstoned row positions and
  predicate mismatches count; rereading the selected row for binding does not.
- effect_operations_completed: one finite registered Assert/Retract operation
  completes its scans, mutation and temporary-disposal precharges. Idempotent
  operations count too.
- choices_restored: a retained branch is successfully popped/restored after
  its restoration precharge; an empty restore attempt does not count.

Entered node reports include their successful precharged stage entry. These are
stage entries/operations, not claims of completed queries or accepted
plans. Failed precharges never count the operation as completed. Consumed search
work and peak logical storage keep their existing accounting meaning.

Run the standalone example with:

```sh
cargo run -p htn-planner --example trace
```

The example explicitly chooses diagnostic capacity 4096 and prints source-linked
reasoning plus the separate complete result. It is a correctness/inspection
example, not a production profile or performance claim. Capture and read-only
lookup/copy overhead must be measured externally with the exact source revision,
workload, profile, platform and active counter/capture settings. Parent190's
qualification measures those separately; the pure runtime reads no wall clock.

`Trace::heap_footprint()` reuses the structural Value walker to report owned node,
locator and argument backing storage. capacity_bytes counts current capacities;
clone_bytes counts current Clone payload lengths. Inline Trace, allocator/Arc
control blocks and traversal scratch are excluded. A host sharing Arc<Trace>
counts that payload as shared immutable storage, while an explicit inspection
copy uses clone_bytes. This is structural accounting rather than allocator/RSS
measurement; the helper is not run by trace capture or read-only lookup itself.

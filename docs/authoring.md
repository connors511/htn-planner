# Authoring `.domain` programs

`htn_planner_ron::domain` compiles supplied text into the typed runtime IR. The
application declares its fact and action signatures, supplies all source text,
and chooses explicit planning limits. Compilation resolves names and types before
planning. Planning returns inert action requests and deferred tasks; the
application owns execution and checks whether each action is still legal.

## Complete example

This source selects an integer fact and requests an action using that value:

```lisp
(:domain example top_level_domain
  (:method (select ?inp_unused) top_level_method
    (emit_candidate
      (candidate ?value)
      ((!emit ?value)))))
```

The following Rust code supplies the complete schema, compiles the source, and
plans against one fact. `Limits::small()` is an example allowance, not a guarantee
that a larger application fits it.

```rust
use htn_planner::{Fact, Limits, Outcome, Schema, Signature, Type, Value, plan};
use htn_planner_ron::domain::{Source, compile};

fn main() -> Result<(), String> {
    let schema = Schema {
        predicates: vec![Signature {
            name: "candidate".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let source = Source {
        id: "example.domain",
        text: r"(:domain example top_level_domain
            (:method (select ?inp_unused) top_level_method
                (emit_candidate (candidate ?value) ((!emit ?value)))))",
    };
    let entry = Signature {
        name: "select".into(),
        arguments: vec![Type::Int],
    };
    let domain = compile(source.id, &[source], schema, &[entry], Limits::small())
        .map_err(|error| error.message)?;
    let facts = domain.snapshot(vec![Fact {
        predicate: "candidate".into(),
        arguments: vec![Value::Int(7)],
    }])?;
    let report = plan(&domain, &facts, "select", &[Value::Int(0)]);
    let Outcome::Success(entries) = report.outcome else {
        return Err(format!("planning failed: {:?}", report.outcome));
    };
    let action = entries[0].as_action().ok_or("expected action")?;
    assert_eq!(action.name, "emit");
    assert_eq!(action.arguments, vec![Value::Int(7)]);
    Ok(())
}
```

The unused entry parameter needs the host's `select(Int)` signature because its
body does not constrain its type. Variable spelling alone does not establish a
type. See the executable [warehouse example](../crates/htn-planner-ron/examples/warehouse.rs)
for a larger supplied-text and RON compatibility journey.

## Declarations and bindings

Each source contains optional includes followed by one `:domain` declaration.
The root domain requires `top_level_domain`; included domains omit that marker.
A root `:method` marked `top_level_method` is a callable entry. Other methods
compose named tasks.

A method contains ordered named branches, each written as
`(branch_name condition (task_calls...))`. Conditions select bindings; task calls
use those bindings. `()` is a successful empty condition or an empty task list,
depending on its position.

Method parameters use `?inp_` names and are bound on entry. They are input-only.
An axiom declares one condition and may also declare `?out_` parameters:

```lisp
(:axiom (select_candidate ?out_value) (candidate ?out_value))
```

Call it with `(#select_candidate ?value)` in a condition. Output arguments must
be distinct fresh variables to yield a solution. A previously bound value or a
literal in an output position makes that call fail; it does not overwrite a
caller binding. Axiom locals remain private to the axiom. Other fresh variables,
such as `?value`, belong to their declaration's local scope. Task output
parameters and `?io_` parameters are unsupported.

The compiler infers types from host signatures and uses. Unconstrained or
inconsistent types reject. A value used by a later call must be bound on every
preceding alternative. Host types include references, records, integers, symbols,
booleans and scaled fixed values; this source profile only spells integer and
symbol literals directly.

## Conditions and calls

| Form | Meaning |
| --- | --- |
| `(and condition...)` | Ordered conjunction. Later failure can reopen earlier witnesses. |
| `(alt condition...)` | Enumerating alternatives in authored order. Later failure can reopen another alternative. |
| `(not condition)` | Grounded negation. Caller variables must already be bound; it introduces no caller bindings. |
| `(predicate arguments...)` | Query a schema-declared fact relation. Fresh variables bind matching values. |
| `(#axiom arguments...)` | Call a declared pure axiom. |
| `(== left right)`, `(!= left right)` | Compare already bound values of matching types. |
| `(< left right)`, `(<= left right)`, `(> left right)`, `(>= left right)` | Ordered comparisons of already bound values; runtime admission checks valid operand types. |
| `(!action arguments...)` | Request a schema-declared action in a task list. |
| `(task arguments...)` | Expand a declared task using bound inputs. |
| `(&task arguments...)` | Retain a deferred task with captured inputs for expansion against a fresh snapshot. |

Negation may call an axiom that uses private existential witnesses. Exhausting
work or capacity while checking negation is an error, never proof of absence.

`alt` does not commit to its first successful child. Source `or` is rejected.
The typed/RON runtime vocabulary separately offers enumerating `Or` and committed
`FirstSuccess`; this language does not alias `or` to either one.

## Constants and supplied includes

Declare integer or symbol constants in named groups and refer to them with `@`:

```lisp
(:constants Timing (delay 90) (mode ready))
```

Use `@delay` or `@mode` as an argument. Group names organize declarations;
constant names resolve across the supplied program and must be unique.

Put `(:include "helpers.domain")` before the root domain and supply a second
`Source { id: "helpers.domain", text: helper_text }` in the same source array.
IDs match the supplied sources; includes never read files or fetch URLs. Included
declarations share the program's name-resolution scope. Missing sources,
duplicate source IDs, include cycles and duplicate declarations reject.

## Rejection and RON compatibility

Unsupported forms reject explicitly. These include `or`, `?io_`, floating-point
and string value literals, arbitrary host calls, planning-effect forms, recursive
tasks or axioms, overloads, overrides and inheritance. Quoted include IDs are
source references, not string expression values. Malformed syntax, unknown
signatures, invalid argument types or binding modes, and invalid limits also
reject before planning. Errors retain source and declaration context; the
frontend's `locate_error` helper can locate downstream admission errors against
the same supplied sources.

RON is a separate explicit API:
`htn_planner_ron::compile(source_id, text, schema, limits)`. Named version 2 is the
recommended RON authoring format; indexed version 1 remains accepted. Neither
`.domain` rejection nor malformed version-2 RON falls back to another format.
See the [frontend README](../crates/htn-planner-ron/README.md) for RON examples
and the [runtime README](../crates/htn-planner/README.md) for outcomes, limits and
deferred identity contracts.

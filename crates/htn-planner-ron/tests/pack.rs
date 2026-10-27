use htn_planner::{FactSnapshot, Limits, Outcome, Value, plan};
#[path = "../examples/support/pack.rs"]
mod fixture;
use fixture::{PACK, row, schema};
#[test_tag::tag(unit)]
#[test]
fn pack_reopens_front_assignment_and_rolls_back_speculative_reservations() {
    let schema = schema();
    let mut limits = Limits::small();
    limits.work = 100_000;
    limits.storage = 65_536;
    limits.bindings = 256;
    let domain = htn_planner_ron::compile("pack", PACK, schema.clone(), limits).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            row("eligible", 1, "front"),
            row("eligible", 2, "front"),
            row("eligible", 3, "left"),
            row("eligible", 1, "right"),
            row("assigned", 9, "bench"),
        ],
    )
    .unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(entries) = &result.outcome else {
        panic!("{result:?}")
    };
    let pairs = entries
        .iter()
        .map(|entry| entry.as_action().unwrap().arguments.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        pairs,
        vec![
            vec![Value::Int(2), Value::Symbol("front".into())],
            vec![Value::Int(3), Value::Symbol("left".into())],
            vec![Value::Int(1), Value::Symbol("right".into())]
        ]
    );
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "entry", &[]));
}

#[test_tag::tag(invariant)]
#[test]
fn impossible_pack_returns_no_prefix_and_preserves_starting_reservations() {
    let schema = schema();
    let mut limits = Limits::small();
    limits.work = 100_000;
    limits.storage = 65_536;
    limits.bindings = 256;
    let domain = htn_planner_ron::compile("pack", PACK, schema.clone(), limits).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            row("eligible", 1, "front"),
            row("eligible", 3, "left"),
            row("assigned", 9, "bench"),
        ],
    )
    .unwrap();
    let before = facts.clone();
    assert_eq!(plan(&domain, &facts, "entry", &[]).outcome, Outcome::NoPlan);
    assert_eq!(facts, before);
}

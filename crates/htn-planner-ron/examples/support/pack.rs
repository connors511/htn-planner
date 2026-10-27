use htn_planner::{
    EffectOperation, Expression, Fact, PlanningEffect, Schema, Signature, Type, Value,
};
pub fn schema() -> Schema {
    Schema {
        predicates: vec![
            Signature {
                name: "eligible".into(),
                arguments: vec![Type::Int, Type::Symbol],
            },
            Signature {
                name: "assigned".into(),
                arguments: vec![Type::Int, Type::Symbol],
            },
        ],
        actions: vec![Signature {
            name: "assign".into(),
            arguments: vec![Type::Int, Type::Symbol],
        }],
        effects: vec![PlanningEffect {
            name: "claim".into(),
            parameters: vec![Type::Int, Type::Symbol],
            operations: vec![EffectOperation::Assert {
                predicate: "assigned".into(),
                arguments: vec![Expression::Parameter(0), Expression::Parameter(1)],
            }],
        }],
        ..Schema::default()
    }
}
pub const PACK: &str = r#"(
    version: 2,
    name: "pack",
    entries: [
        "entry",
    ],
    queries: [
        (
            name: "taken",
            parameters: [
                Int("actor",),
            ],
            outputs: [],
            locals: [
                Symbol("role",),
            ],
            condition: Fact(
                name: "assigned",
                arguments: [
                    Var("actor",),
                    Var("role",),
                ],
            ),
            result: [],
        ),
        (
            name: "role_taken",
            parameters: [
                Symbol("role",),
            ],
            outputs: [],
            locals: [
                Int("actor",),
            ],
            condition: Fact(
                name: "assigned",
                arguments: [
                    Var("actor",),
                    Var("role",),
                ],
            ),
            result: [],
        ),
        (
            name: "candidate",
            parameters: [
                Symbol("role",),
            ],
            outputs: [
                Int,
            ],
            locals: [
                Int("actor",),
            ],
            condition: And([
                Fact(
                    name: "eligible",
                    arguments: [
                        Var("actor",),
                        Var("role",),
                    ],
                ),
                Not(Derived(
                    name: "taken",
                    arguments: [
                        Var("actor",),
                    ],
                    outputs: [],
                ),),
                Not(Derived(
                    name: "role_taken",
                    arguments: [
                        Var("role",),
                    ],
                    outputs: [],
                ),),
            ],),
            result: [
                Var("actor",),
            ],
        ),
    ],
    tasks: [
        (
            name: "entry",
            parameters: [],
            methods: [
                (
                    name: "roles",
                    steps: [
                        Task(
                            name: "role",
                            arguments: [
                                Literal(Symbol("front",),),
                            ],
                        ),
                        Task(
                            name: "role",
                            arguments: [
                                Literal(Symbol("left",),),
                            ],
                        ),
                        Task(
                            name: "role",
                            arguments: [
                                Literal(Symbol("right",),),
                            ],
                        ),
                        Condition(Not(Fact(
                            name: "assigned",
                            arguments: [
                                Literal(Int(1,),),
                                Literal(Symbol("front",),),
                            ],
                        ),),),
                    ],
                    locals: [],
                ),
            ],
        ),
        (
            name: "role",
            parameters: [
                Symbol("role",),
            ],
            methods: [
                (
                    name: "assign",
                    locals: [
                        Int("actor",),
                    ],
                    steps: [
                        Condition(Derived(
                            name: "candidate",
                            arguments: [
                                Var("role",),
                            ],
                            outputs: [
                                Var("actor",),
                            ],
                        ),),
                        Effect(
                            name: "claim",
                            arguments: [
                                Var("actor",),
                                Var("role",),
                            ],
                        ),
                        Action(
                            name: "assign",
                            arguments: [
                                Var("actor",),
                                Var("role",),
                            ],
                        ),
                    ],
                ),
            ],
        ),
    ],
)"#;
pub fn row(name: &str, actor: i64, role: &str) -> Fact {
    Fact {
        predicate: name.into(),
        arguments: vec![Value::Int(actor), Value::Symbol(role.into())],
    }
}

use super::validate_values;
use crate::{Outcome, RecordType, Schema, Type, Value};
use test_tag::tag;

fn fixture() -> (Schema, Vec<Type>, Vec<Value>) {
    let schema = Schema {
        references: vec!["Actor".into()],
        records: vec![RecordType {
            name: "R".into(),
            fields: vec![Type::Symbol, Type::Ref("Actor".into())],
        }],
        ..Schema::default()
    };
    let types = vec![Type::Record("R".into()), Type::Symbol];
    let values = vec![
        Value::Record {
            type_name: "R".into(),
            fields: vec![
                Value::Symbol("first".into()),
                Value::Ref {
                    type_name: "Actor".into(),
                    key: 7,
                },
            ],
        },
        Value::Symbol("tail".into()),
    ];
    (schema, types, values)
}

// Exact units/order/peaks preserve the public planner's existing budget and error contract.
const CHARGES: &[usize] = &[1, 1, 2, 3, 2, 1, 10, 11, 1, 5, 1, 4];

#[test]
#[tag(compatibility)]
fn mixed_arguments_keep_lifo_validation_costs_and_logical_peak() {
    let (schema, types, values) = fixture();
    let mut charged = Vec::new();
    let mut peak = 0;
    let result = validate_values(&schema, &types, &values, 3, &mut peak, |units| {
        charged.push(units);
        Ok(())
    });
    assert_eq!(result, Ok(vec![14, 5]));
    assert_eq!(charged, CHARGES);
    assert_eq!(peak, 3);
}

#[test]
#[tag(compatibility)]
fn fuel_failure_keeps_the_same_charge_frontier_across_arguments() {
    let (schema, types, values) = fixture();
    let total: usize = CHARGES.iter().sum();
    for fuel in 0..=total + 1 {
        let mut used = 0usize;
        let mut charged = Vec::new();
        let mut peak = 0;
        let result = validate_values(&schema, &types, &values, 3, &mut peak, |units| {
            charged.push(units);
            if used.saturating_add(units) > fuel {
                return Err(Outcome::BudgetExhausted);
            }
            used += units;
            Ok(())
        });
        let mut expected_used = 0usize;
        let expected_len = CHARGES
            .iter()
            .position(|units| {
                expected_used += units;
                expected_used > fuel
            })
            .map_or(CHARGES.len(), |index| index + 1);
        assert_eq!(charged, CHARGES[..expected_len], "fuel {fuel}");
        assert_eq!(
            result,
            if fuel < total {
                Err(Outcome::BudgetExhausted)
            } else {
                Ok(vec![14, 5])
            },
            "fuel {fuel}"
        );
        assert!(peak <= 3);
    }
}

#[test]
#[tag(compatibility)]
fn malformed_record_stops_in_the_same_field_before_later_arguments() {
    let (schema, types, mut values) = fixture();
    let Value::Record { fields, .. } = &mut values[0] else {
        unreachable!()
    };
    fields[1] = Value::Ref {
        type_name: "wrong".into(),
        key: 7,
    };
    let mut charged = Vec::new();
    let mut peak = 0;
    let result = validate_values(&schema, &types, &values, 3, &mut peak, |units| {
        charged.push(units);
        Ok(())
    });
    assert_eq!(result, Err(Outcome::InvalidInput));
    assert_eq!(charged, [1, 1, 2, 3, 2, 1, 10]);
    assert_eq!(peak, 3);
}

#[test]
#[tag(compatibility)]
fn storage_rejection_precedes_argument_work_and_retained_capacity_is_not_a_logical_peak() {
    let (schema, types, values) = fixture();
    let mut charged = Vec::new();
    let mut peak = 0;
    let rejected = validate_values(&schema, &types, &values, 2, &mut peak, |units| {
        charged.push(units);
        Ok(())
    });
    assert_eq!(rejected, Err(Outcome::CapacityExceeded));
    assert_eq!(charged, [1]);
    assert_eq!(peak, 0);
    peak = 17;
    assert_eq!(
        validate_values(&schema, &types, &values, 3, &mut peak, |_| Ok(())),
        Ok(vec![14, 5])
    );
    assert_eq!(peak, 17);
}

mod overflow;

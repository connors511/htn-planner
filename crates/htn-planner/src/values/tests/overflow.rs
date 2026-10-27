use super::*;

#[test]
#[tag(compatibility)]
fn inline_and_overflow_widths_keep_lifo_costs_charges_and_logical_peak() {
    for width in 0..=12 {
        let fields: Vec<_> = (1..=width).map(|n| Value::Symbol("x".repeat(n))).collect();
        let schema = Schema {
            records: vec![RecordType {
                name: "R".into(),
                fields: vec![Type::Symbol; width],
            }],
            ..Schema::default()
        };
        let mut charges = vec![];
        let mut peak = 0;
        let result = validate_values(
            &schema,
            &[Type::Record("R".into())],
            &[Value::Record {
                type_name: "R".into(),
                fields,
            }],
            width.max(1) + 1,
            &mut peak,
            |n| {
                charges.push(n);
                Ok(())
            },
        );
        // Exact traversal/fuel units and logical peak are compatibility contracts.
        let mut expected = vec![1, 1, 2, 3, width];
        for n in (1..=width).rev() {
            expected.extend([1, n]);
        }
        assert_eq!(charges, expected, "width {width}");
        assert_eq!(
            result,
            Ok(vec![2 + width + width * (width + 1) / 2]),
            "width {width}"
        );
        assert_eq!(peak, width + 1, "width {width}");
    }
}
fn nested_fixture() -> (Schema, Vec<Value>) {
    let schema = Schema {
        records: vec![
            RecordType {
                name: "Root".into(),
                fields: (0..5)
                    .map(|_| Type::Symbol)
                    .chain([Type::Record("Leaf".into())])
                    .collect(),
            },
            RecordType {
                name: "Leaf".into(),
                fields: vec![Type::Symbol; 6],
            },
        ],
        ..Schema::default()
    };
    let fields = (101..=105)
        .map(|n| Value::Symbol("x".repeat(n)))
        .chain([Value::Record {
            type_name: "Leaf".into(),
            fields: (106..=111).map(|n| Value::Symbol("x".repeat(n))).collect(),
        }])
        .collect();
    (
        schema,
        vec![Value::Record {
            type_name: "Root".into(),
            fields,
        }],
    )
}
#[test]
#[tag(compatibility)]
fn nested_overflow_retains_outer_pending_fields_and_exact_storage_boundary() {
    let (schema, values) = nested_fixture();
    for limit in [11, 12] {
        let mut peak = 0;
        let mut payloads = vec![];
        let result = validate_values(
            &schema,
            &[Type::Record("Root".into())],
            &values,
            limit,
            &mut peak,
            |n| {
                if n >= 100 {
                    payloads.push(n);
                }
                Ok(())
            },
        );
        if limit == 11 {
            assert_eq!(result, Err(Outcome::CapacityExceeded));
            assert!(payloads.is_empty());
            assert_eq!(peak, 7);
        } else {
            assert_eq!(result, Ok(vec![1187]));
            assert_eq!(payloads, (101..=111).rev().collect::<Vec<_>>());
            assert_eq!(peak, 12);
        }
    }
}
#[test]
#[tag(compatibility)]
fn malformed_nested_overflow_fails_before_earlier_outer_fields() {
    let (schema, mut values) = nested_fixture();
    let Value::Record { fields, .. } = &mut values[0] else {
        unreachable!()
    };
    fields[0] = Value::Bool(false);
    let Value::Record { fields, .. } = &mut fields[5] else {
        unreachable!()
    };
    fields[5] = Value::Bool(false);
    let mut peak = 0;
    let mut payloads = vec![];
    assert_eq!(
        validate_values(
            &schema,
            &[Type::Record("Root".into())],
            &values,
            12,
            &mut peak,
            |n| {
                if n >= 100 {
                    payloads.push(n);
                }
                Ok(())
            }
        ),
        Err(Outcome::InvalidInput)
    );
    assert_eq!(peak, 12);
    assert!(
        payloads.is_empty(),
        "last nested field must be visited before other leaves"
    );
}

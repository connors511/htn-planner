//! Failure-only lookup maps typed downstream errors to supplied authoring text.
use htn_planner::{Error, Locator};
use htn_planner_ron::domain::{Source, locate_error};

const ROOT: &str = r#"(:include "included/Helpers.domain")
(:domain Root top_level_domain
    (:method (ready ?inp_value) top_level_method
        (entry () ((helper ?inp_value)))))"#;
const HELPER: &str = r"(:domain Helpers
    (:method (helper ?inp_value)
        (choose
            (and (eligible ?inp_value))
            ((!emit ?inp_value)
             (finish ?inp_value))))
    (:method (finish ?inp_value)
        (done () ())))";

const fn sources() -> [Source<'static>; 2] {
    [
        Source {
            id: "Root.domain",
            text: ROOT,
        },
        Source {
            id: "included/Helpers.domain",
            text: HELPER,
        },
    ]
}

fn downstream_error(step: Option<usize>) -> Error {
    Error {
        locator: Locator {
            source: "lowered-registry".into(),
            task: Some("helper".into()),
            declaration: "choose".into(),
            step,
        },
        message: "downstream argument constraint".into(),
    }
}

fn position(marker: &str) -> Option<(usize, usize)> {
    HELPER
        .lines()
        .enumerate()
        .find_map(|(line, text)| text.find(marker).map(|column| (line + 1, column + 1)))
}

#[test_tag::tag(unit)]
#[test]
fn included_condition_zero_and_call_steps_resolve_their_actual_source_positions() {
    // A downstream method's step zero denotes its condition. Calls start at one,
    // so the first call cannot accidentally point at the second call or root text.
    for (step, marker) in [(0, "(and"), (1, "(!emit"), (2, "(finish")] {
        let original = downstream_error(Some(step));
        let located = locate_error("Root.domain", &sources(), original.clone());
        let (line, column) =
            position(marker).expect("fixture marker must identify its source node");
        assert_eq!(located.locator.source, "included/Helpers.domain");
        assert_eq!(located.locator.task, original.locator.task);
        assert_eq!(located.locator.step, original.locator.step);
        assert_eq!(located.message, original.message);
        assert!(located.locator.declaration.contains("choose"));
        assert!(
            located
                .locator
                .declaration
                .contains(&format!("{line}:{column}")),
            "wrong source position for step {step}: {located:?}"
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn branch_error_without_a_step_resolves_the_included_branch() {
    let original = downstream_error(None);
    let located = locate_error("Root.domain", &sources(), original.clone());
    let (line, column) = position("choose").expect("fixture branch must exist");
    assert_eq!(located.locator.source, "included/Helpers.domain");
    assert_eq!(located.locator.task, original.locator.task);
    assert_eq!(located.locator.step, None);
    assert_eq!(located.message, original.message);
    assert!(located.locator.declaration.contains("choose"));
    assert!(
        located
            .locator
            .declaration
            .contains(&format!("{line}:{column}"))
    );
}

#[test_tag::tag(unit)]
#[test]
fn absent_or_unknown_task_preserves_the_complete_original_error() {
    for task in [None, Some("missing_task")] {
        let mut original = downstream_error(Some(0));
        original.locator.task = task.map(str::to_owned);
        assert_eq!(
            locate_error("Root.domain", &sources(), original.clone()),
            original
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn unknown_branch_preserves_the_complete_original_error() {
    let mut original = downstream_error(Some(0));
    original.locator.declaration = "missing_branch".into();
    assert_eq!(
        locate_error("Root.domain", &sources(), original.clone()),
        original,
        "an unknown branch cannot invent a source position from the task header"
    );
}

#[test_tag::tag(unit)]
#[test]
fn out_of_range_step_preserves_the_complete_original_error() {
    let original = downstream_error(Some(usize::MAX));
    assert_eq!(
        locate_error("Root.domain", &sources(), original.clone()),
        original,
        "an unmapped call cannot invent a source position from the task header"
    );
}

#[test_tag::tag(unit)]
#[test]
fn invalid_supplied_source_graphs_preserve_the_complete_original_error() {
    let complete = sources();
    let malformed = Source {
        id: "Root.domain",
        text: "(:domain",
    };
    let cyclic_helper = Source {
        id: "included/Helpers.domain",
        text: r#"(:include "Root.domain") (:domain Helpers)"#,
    };
    for invalid in [
        vec![complete[0]],
        vec![complete[0], complete[0], complete[1]],
        vec![malformed],
        vec![complete[0], cyclic_helper],
    ] {
        let original = downstream_error(Some(1));
        assert_eq!(
            locate_error("Root.domain", &invalid, original.clone()),
            original
        );
    }
}

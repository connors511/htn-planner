//! Failure-only source lookup for diagnostics from typed downstream admission.
use super::{Source, declarations, types::Types};
use htn_planner::Error;

/// Resolve a task/branch/step error against the same supplied sources used to lower it.
///
/// This bounded cold lookup runs only when an adapter rejects a lowered definition;
/// no source map enters the executable IR, identity or runtime state.
/// Unknown locators or invalid source graphs retain the original error.
pub fn locate_error(root: &str, sources: &[Source<'_>], mut error: Error) -> Error {
    let Some(task) = error.locator.task.as_deref() else {
        return error;
    };
    let Ok(program) = declarations::collect(root, sources, &[], &mut Types::default()) else {
        return error;
    };
    let Some(declaration) = program.tasks.iter().find(|decl| decl.name == task) else {
        return error;
    };
    let Some(branch) = declaration.body.iter().find_map(|node| {
        let form = node.list()?;
        (form.first()?.atom()? == error.locator.declaration).then_some(form)
    }) else {
        return error;
    };
    let node = match error.locator.step {
        Some(0) => branch.get(1),
        Some(step) => branch
            .get(2)
            .and_then(|node| node.list())
            .and_then(|nodes| nodes.get(step - 1)),
        None => branch.first(),
    };
    let Some(node) = node else {
        return error;
    };
    error.locator.source = node.span.source.to_string();
    error.locator.declaration = format!(
        "{} (line {}:{})",
        error.locator.declaration, node.span.line, node.span.column
    );
    error
}

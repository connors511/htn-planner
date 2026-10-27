//! Pure, schema-typed admission of the deterministic `.domain` language profile.
use htn_planner::{CompiledDomain, Definition, Error, Limits, Locator, Schema, Signature};
mod declarations;
mod diagnostics;
pub use diagnostics::locate_error;
mod lower;
mod syntax;
mod types;

/// One immutable source supplied by the host. Includes never access the filesystem.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub id: &'a str,
    pub text: &'a str,
}

/// Lower named authoring to typed planner IR.
/// # Errors
/// Rejects malformed, unsupported, ambiguous or unresolved authoring.
pub fn lower(
    root_id: &str,
    sources: &[Source<'_>],
    schema: &Schema,
    entry_signatures: &[Signature],
) -> Result<Definition, Error> {
    schema
        .validate_ref()
        .map_err(|message| error(root_id, &message))?;
    lower_validated(root_id, sources, schema, entry_signatures)
}

fn lower_validated(
    root_id: &str,
    sources: &[Source<'_>],
    schema: &Schema,
    entry_signatures: &[Signature],
) -> Result<Definition, Error> {
    if entry_signatures.len() > 64 {
        return Err(error(root_id, "entry signature capacity exceeded"));
    }
    for signature in entry_signatures {
        if !lower::valid_name(&signature.name) || signature.arguments.len() > 1024 {
            return Err(error(root_id, "invalid or oversized entry signature"));
        }
        for ty in &signature.arguments {
            let known = match ty {
                htn_planner::Type::Ref(name) => {
                    name.len() <= 1024 && schema.references.contains(name)
                }
                htn_planner::Type::Record(name) => {
                    name.len() <= 1024 && schema.records.iter().any(|record| record.name == *name)
                }
                htn_planner::Type::Fixed(scale) => *scale <= 18,
                _ => true,
            };
            if !known {
                return Err(error(root_id, "unknown or invalid entry binding type"));
            }
        }
    }
    let mut types = types::Types::default();
    let program = declarations::collect(root_id, sources, entry_signatures, &mut types)?;
    lower::lower(&program, schema, &mut types)
}

fn error(source: &str, message: &str) -> Error {
    Error {
        locator: Locator {
            source: source.chars().take(1024).collect(),
            task: None,
            declaration: "domain".into(),
            step: None,
        },
        message: message.into(),
    }
}

/// Parse, infer types and admit a source graph against the host schema and limits.
/// # Errors
/// Rejects invalid source graphs, bindings, types or executable definitions.
pub fn compile(
    root_id: &str,
    sources: &[Source<'_>],
    schema: Schema,
    entry_signatures: &[Signature],
    limits: Limits,
) -> Result<CompiledDomain, Error> {
    // Validate the owned input before any early source rejection, so malformed
    // effect literal trees receive the runtime's iterative disposal guard.
    let schema = schema
        .validate()
        .map_err(|message| error(root_id, &message))?;
    let definition = lower_validated(root_id, sources, &schema, entry_signatures)?;
    CompiledDomain::admit(root_id, schema, definition, limits)
        .map_err(|error| locate_error(root_id, sources, error))
}

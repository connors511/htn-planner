//! Schema-typed `.domain` and compatible RON admission from caller-supplied text.
//! Parsing never loads files; use [`domain`] for the deterministic language profile.
use htn_planner::{CompiledDomain, Definition, Error, Limits, Locator, Schema};
use serde::Deserialize;

pub mod authoring;
pub mod domain;

#[derive(Deserialize)]
struct Header {
    version: u32,
}

fn parse<T: serde::de::DeserializeOwned>(source: &str, text: &str) -> Result<T, Error> {
    ron::from_str(text).map_err(|error| Error {
        locator: Locator {
            source: source.into(),
            task: None,
            declaration: format!("{}:{}", error.position.line, error.position.col),
            step: None,
        },
        message: error.code.to_string(),
    })
}

/// Parse and admit a domain, retaining the explicit source identity.
///
/// # Errors
/// Rejects malformed RON (including parser recursion limits) and invalid definitions.
pub fn compile(
    source_id: &str,
    text: &str,
    schema: Schema,
    limits: Limits,
) -> Result<CompiledDomain, Error> {
    let schema = schema.validate().map_err(|message| Error {
        locator: Locator {
            task: None,
            source: if source_id.len() <= 1024 {
                source_id.into()
            } else {
                "<oversized source>".into()
            },
            declaration: "<schema>".into(),
            step: None,
        },
        message,
    })?;
    if source_id.len() > 1024 || text.len() > 1_000_000 {
        return Err(Error {
            locator: Locator {
                task: None,
                source: "<oversized source>".into(),
                declaration: "<domain>".into(),
                step: None,
            },
            message: "source identity or text capacity exceeded".into(),
        });
    }
    let header: Header = parse(source_id, text)?;
    let definition: Definition = match header.version {
        1 => parse(source_id, text)?,
        2 => parse::<authoring::Definition>(source_id, text)?.lower(source_id)?,
        _ => {
            return Err(Error {
                locator: Locator {
                    source: source_id.into(),
                    task: None,
                    declaration: "<domain>".into(),
                    step: None,
                },
                message: "unsupported authoring version".into(),
            });
        }
    };
    CompiledDomain::admit(source_id, schema, definition, limits)
}

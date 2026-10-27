use super::{CompiledCondition, CompiledExpression, MethodCompiler, error};
use crate::{Definition, EffectOperation, Error, Expression, Limits, Method, Schema, Task};
#[derive(Clone, Debug)]
pub(crate) struct CompiledEffect {
    pub(crate) operations: Vec<CompiledOperation>,
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledOperation {
    pub(crate) assert: bool,
    pub(crate) predicate: usize,
    pub(crate) arguments: Vec<CompiledExpression>,
}
pub(super) fn compile_effects(
    source: &str,
    schema: &Schema,
    definition: &Definition,
    limits: Limits,
    size: &mut usize,
    arena: &mut Vec<CompiledCondition>,
) -> Result<Vec<CompiledEffect>, Error> {
    let mut effects = Vec::new();
    let mut names = Vec::new();
    for effect in &schema.effects {
        if effect.name.is_empty()
            || effect.name.len() > 1024
            || names.contains(&effect.name.as_str())
        {
            return Err(error(
                source,
                &effect.name,
                None,
                "duplicate or invalid effect",
            ));
        }
        names.push(effect.name.as_str());
        if effect.parameters.len() > limits.bindings || effect.operations.len() > 16384 {
            return Err(error(
                source,
                &effect.name,
                None,
                "effect capacity exceeded",
            ));
        }
        let task = Task {
            name: effect.name.clone(),
            parameters: effect.parameters.clone(),
            outputs: vec![],
            methods: vec![],
        };
        let method = Method {
            name: effect.name.clone(),
            locals: vec![],
            result: vec![],
            steps: vec![],
        };
        let mut context = MethodCompiler {
            source,
            schema,
            definition,
            task: &task,
            method: &method,
            bound: vec![],
            size,
            step: None,
            arena,
            max_depth: limits.depth,
        };
        let mut operations = Vec::new();
        for (index, operation) in effect.operations.iter().enumerate() {
            context.step = Some(index);
            let (assert, name, arguments) = match operation {
                EffectOperation::Assert {
                    predicate,
                    arguments,
                } => (true, predicate, arguments),
                EffectOperation::Retract {
                    predicate,
                    arguments,
                } => (false, predicate, arguments),
            };
            if arguments
                .iter()
                .any(|arg| matches!(arg, Expression::Local(_)))
            {
                return Err(context.fail("effect operation cannot use local slots"));
            }
            let predicate = schema
                .predicates
                .iter()
                .position(|p| p.name == *name)
                .ok_or_else(|| context.fail("unknown effect predicate"))?;
            operations.push(CompiledOperation {
                assert,
                predicate,
                arguments: context.expressions(
                    arguments,
                    &schema.predicates[predicate].arguments,
                    false,
                )?,
            });
        }
        effects.push(CompiledEffect { operations });
    }
    Ok(effects)
}

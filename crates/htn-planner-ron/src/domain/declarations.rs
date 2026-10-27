use super::{
    Source,
    lower::{atom, list, valid_name},
    syntax::{self, Kind, Node},
    types::Types,
};
use htn_planner::{Error, Signature, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Parameter {
    pub name: String,
    pub id: usize,
    pub output: bool,
}
pub(super) struct Declaration {
    pub at: Node,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub body: Vec<Node>,
    pub entry: bool,
}
pub(super) struct Program {
    pub name: String,
    pub tasks: Vec<Declaration>,
    pub queries: Vec<Declaration>,
    pub constants: BTreeMap<String, Value>,
}

/// Resolve only an explicit immutable source set. Include order is declaration order.
pub(super) fn collect(
    root: &str,
    sources: &[Source<'_>],
    hints: &[Signature],
    types: &mut Types,
) -> Result<Program, Error> {
    if sources.len() > 64
        || sources
            .iter()
            .map(|source| source.text.len())
            .sum::<usize>()
            > 4_000_000
    {
        return Err(super::error(root, "source graph capacity exceeded"));
    }
    let mut source_map = BTreeMap::new();
    for source in sources {
        if source.id.is_empty()
            || source.id.len() > 1024
            || source
                .id
                .split('/')
                .any(|part| part == ".." || part.is_empty())
            || source_map.insert(source.id, *source).is_some()
        {
            return Err(super::error(root, "invalid or duplicate source identity"));
        }
    }
    let mut forms = Vec::new();
    visit(
        root,
        &source_map,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut forms,
        &mut 100_000,
    )?;
    let root_node = forms
        .iter()
        .find(|node| node.span.source.as_ref() == root)
        .ok_or_else(|| super::error(root, "missing root domain"))?
        .clone();
    let mut program = Program {
        name: String::new(),
        tasks: vec![],
        queries: vec![],
        constants: BTreeMap::new(),
    };
    for form in forms {
        let domain = list(&form)?;
        if domain.len() < 2 || atom(&domain[0])? != ":domain" {
            return Err(form.error("expected domain declaration"));
        }
        let name = atom(&domain[1])?;
        if !valid_name(name) {
            return Err(domain[1].error("invalid domain name"));
        }
        let top = domain.get(2).and_then(Node::atom) == Some("top_level_domain");
        if form.span.source.as_ref() == root {
            if !top {
                return Err(form.error("root domain requires top_level_domain"));
            }
            program.name = name.into();
        } else if top {
            return Err(form.error("included domain cannot be top_level_domain"));
        }
        for node in &domain[if top { 3 } else { 2 }..] {
            let decl = list(node)?;
            let head = atom(
                decl.first()
                    .ok_or_else(|| node.error("empty declaration"))?,
            )?;
            match head {
                ":constants" => {
                    if decl.len() < 2 || !valid_name(atom(&decl[1])?) {
                        return Err(node.error("expected named constant group"));
                    }
                    for item in &decl[2..] {
                        let pair = list(item)?;
                        if pair.len() != 2 || !valid_name(atom(&pair[0])?) {
                            return Err(item.error("expected (constant literal)"));
                        }
                        let name = atom(&pair[0])?;
                        let value = literal(&pair[1])?;
                        if program.constants.insert(name.into(), value).is_some() {
                            return Err(item.error("duplicate constant"));
                        }
                    }
                }
                ":method" | ":axiom" => {
                    if decl.len() < 2 {
                        return Err(node.error("missing declaration head"));
                    }
                    let signature = list(&decl[1])?;
                    let name = atom(
                        signature
                            .first()
                            .ok_or_else(|| node.error("empty declaration head"))?,
                    )?;
                    if !valid_name(name) {
                        return Err(node.error("invalid declaration name"));
                    }
                    let target = if head == ":method" {
                        &mut program.tasks
                    } else {
                        &mut program.queries
                    };
                    if target.iter().any(|previous| previous.name == name) {
                        return Err(node.error(
                            "duplicate declaration; overloads and overrides are unsupported",
                        ));
                    }
                    let mut parameters: Vec<Parameter> = Vec::new();
                    for param in &signature[1..] {
                        let name = atom(param)?;
                        let output = head == ":axiom" && name.starts_with("?out_");
                        if !(name.starts_with("?inp_") || output)
                            || !name.strip_prefix('?').is_some_and(valid_name)
                            || parameters.iter().any(|previous| previous.name == name)
                        {
                            return Err(param.error("expected distinct ?inp_ parameters (axioms also support fresh ?out_); ?io_ is unsupported"));
                        }
                        parameters.push(Parameter {
                            name: name.into(),
                            id: types.fresh(),
                            output,
                        });
                    }
                    let entry = decl.get(2).and_then(Node::atom) == Some("top_level_method");
                    if entry && (head != ":method" || !top) {
                        return Err(node.error("only root methods may be top_level_method"));
                    }
                    if let Some(hint) = hints.iter().find(|hint| hint.name == name) {
                        if !entry || hint.arguments.len() != parameters.len() {
                            return Err(node.error("host signature must name a matching entry"));
                        }
                        for (parameter, ty) in parameters.iter().zip(&hint.arguments) {
                            types.bind(parameter.id, ty, node)?;
                        }
                    }
                    let body = decl[if entry { 3 } else { 2 }..].to_vec();
                    if head == ":axiom" && body.len() != 1 {
                        return Err(node.error("axiom requires one condition"));
                    }
                    target.push(Declaration {
                        at: node.clone(),
                        name: name.into(),
                        parameters,
                        body,
                        entry,
                    });
                }
                _ => return Err(node.error(format!("unsupported declaration {head}"))),
            }
        }
    }
    let mut names = BTreeSet::new();
    for hint in hints {
        if !names.insert(&hint.name)
            || !program
                .tasks
                .iter()
                .any(|task| task.entry && task.name == hint.name)
        {
            return Err(root_node.error("duplicate or unknown host entry signature"));
        }
    }
    if program.tasks.len() > 64 || program.queries.len() > 64 || program.constants.len() > 256 {
        return Err(root_node.error("declaration capacity exceeded"));
    }
    Ok(program)
}

pub(super) fn literal(node: &Node) -> Result<Value, Error> {
    let text = atom(node)?;
    if let Ok(number) = text.parse::<i64>() {
        return Ok(Value::Int(number));
    }
    if valid_name(text) {
        return Ok(Value::Symbol(text.into()));
    }
    Err(node.error("unsupported literal; expected signed integer or symbol"))
}

fn visit(
    id: &str,
    sources: &BTreeMap<&str, Source<'_>>,
    active: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
    forms: &mut Vec<Node>,
    remaining_nodes: &mut usize,
) -> Result<(), Error> {
    if active.contains(id) {
        return Err(super::error(id, "include cycle"));
    }
    if done.contains(id) {
        return Ok(());
    }
    let source = sources
        .get(id)
        .ok_or_else(|| super::error(id, "missing included source"))?;
    active.insert(id.into());
    let nodes = syntax::parse(id, source.text)?;
    let mut pending: Vec<_> = nodes.iter().collect();
    while let Some(node) = pending.pop() {
        *remaining_nodes = remaining_nodes
            .checked_sub(1)
            .ok_or_else(|| node.error("source graph node capacity exceeded"))?;
        if let Some(children) = node.list() {
            pending.extend(children);
        }
    }
    let mut domain = None;
    for node in nodes {
        let form = list(&node)?;
        match form.first().and_then(Node::atom) {
            Some(":include") => {
                let [_, path] = form else {
                    return Err(node.error("include requires one quoted source identity"));
                };
                let Kind::String(path) = &path.kind else {
                    return Err(path.error("include requires quoted source identity"));
                };
                if path.split('/').any(|part| part == ".." || part.is_empty()) {
                    return Err(node.error("invalid include identity"));
                }
                visit(path, sources, active, done, forms, remaining_nodes)
                    .map_err(|error| node.error(format!("include {path:?}: {}", error.message)))?;
            }
            Some(":domain") if domain.is_none() => domain = Some(node),
            _ => return Err(node.error("expected includes followed by one domain")),
        }
    }
    forms.push(domain.ok_or_else(|| super::error(id, "missing domain declaration"))?);
    active.remove(id);
    done.insert(id.into());
    Ok(())
}

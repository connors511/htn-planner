//! Bounded cold S-expression syntax; atoms acquire meaning during lowering.
use htn_planner::{Error, Locator};
use std::sync::Arc;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_SOURCE: usize = 1024;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub source: Arc<str>,
    pub line: usize,
    pub column: usize,
}
impl Span {
    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            locator: Locator {
                source: self.source.to_string(),
                declaration: format!("line {}:{}", self.line, self.column),
                task: None,
                step: None,
            },
            message: message.into(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub span: Span,
    pub kind: Kind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Atom(String),
    String(String),
    List(Vec<Node>),
}
impl Node {
    pub fn atom(&self) -> Option<&str> {
        match &self.kind {
            Kind::Atom(atom) => Some(atom),
            Kind::String(_) | Kind::List(_) => None,
        }
    }
    pub fn list(&self) -> Option<&[Self]> {
        match &self.kind {
            Kind::List(nodes) => Some(nodes),
            Kind::Atom(_) | Kind::String(_) => None,
        }
    }
    pub fn error(&self, message: impl Into<String>) -> Error {
        self.span.error(message)
    }
}

pub(crate) fn parse(source: &str, text: &str) -> Result<Vec<Node>, Error> {
    if source.len() > MAX_SOURCE || text.len() > MAX_BYTES {
        return Err(Span {
            source: if source.len() <= MAX_SOURCE {
                source.into()
            } else {
                "<oversized source>".into()
            },
            line: 1,
            column: 1,
        }
        .error("S-expression source capacity exceeded"));
    }
    let mut parser = Parser {
        source: source.into(),
        text,
        offset: 0,
        line: 1,
        column: 1,
        nodes: 0,
    };
    let mut forms = Vec::new();
    parser.skip();
    while parser.peek().is_some() {
        forms.push(parser.node(0)?);
        parser.skip();
    }
    Ok(forms)
}

struct Parser<'a> {
    source: Arc<str>,
    text: &'a str,
    offset: usize,
    line: usize,
    column: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.offset..].chars().next()
    }
    fn bump(&mut self) {
        let Some(ch) = self.peek() else {
            return;
        };
        self.offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }
    fn span(&self) -> Span {
        Span {
            source: self.source.clone(),
            line: self.line,
            column: self.column,
        }
    }
    fn comment(&self) -> bool {
        self.text[self.offset..].starts_with("//")
    }
    fn skip(&mut self) {
        loop {
            if self.comment() {
                while self.peek().is_some_and(|ch| ch != '\n') {
                    self.bump();
                }
            } else if self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            } else {
                break;
            }
        }
    }
    fn node(&mut self, depth: usize) -> Result<Node, Error> {
        let span = self.span();
        if self.nodes == MAX_NODES {
            return Err(span.error("S-expression node capacity exceeded"));
        }
        self.nodes += 1;
        let kind = match self.peek() {
            Some('(') => {
                if depth >= MAX_DEPTH {
                    return Err(span.error("S-expression nesting capacity exceeded"));
                }
                self.bump();
                let mut children = Vec::new();
                loop {
                    self.skip();
                    match self.peek() {
                        Some(')') => {
                            self.bump();
                            break;
                        }
                        None => return Err(span.error("unterminated list")),
                        Some(_) => children.push(self.node(depth + 1)?),
                    }
                }
                Kind::List(children)
            }
            Some(')') => return Err(span.error("unmatched closing parenthesis")),
            Some('"') => {
                self.bump();
                let start = self.offset;
                loop {
                    match self.peek() {
                        Some('"') => {
                            let value = self.text[start..self.offset].into();
                            self.bump();
                            break Kind::String(value);
                        }
                        None => return Err(span.error("unterminated string")),
                        Some(_) => {
                            self.bump();
                        }
                    }
                }
            }
            Some(_) => {
                let start = self.offset;
                while self
                    .peek()
                    .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, '(' | ')' | '"'))
                    && !self.comment()
                {
                    self.bump();
                }
                Kind::Atom(self.text[start..self.offset].into())
            }
            None => return Err(span.error("expected form")),
        };
        Ok(Node { span, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_tag::tag;

    #[test]
    #[tag(unit)]
    fn parses_nested_forms_comments_and_unescaped_strings_with_locations() {
        let forms = parse("tiny.domain", "// comment\n(root ?target (name \"a\\b\"))").unwrap();
        let root = forms[0].list().unwrap();
        assert_eq!(root[0].atom(), Some("root"));
        assert_eq!(root[1].atom(), Some("?target"));
        let name = root[2].list().unwrap();
        assert_eq!(name[1].kind, Kind::String("a\\b".into()));
        assert_eq!(forms[0].span.line, 2);
        assert_eq!(forms[0].span.column, 1);
        assert_eq!(root[0].span.column, 2);
        let error = root[1].error("unknown variable");
        assert_eq!(error.locator.source, "tiny.domain");
        assert_eq!(error.locator.declaration, "line 2:7");
        assert_eq!(error.message, "unknown variable");
    }

    #[test]
    #[tag(unit)]
    fn empty_sources_and_empty_lists_are_valid_forms() {
        assert!(parse("empty", " // comment").unwrap().is_empty());
        let forms = parse("lists", "() atom ()").unwrap();
        assert!(forms[0].list().unwrap().is_empty());
        assert_eq!(forms[1].atom(), Some("atom"));
        assert!(forms[2].list().unwrap().is_empty());
    }

    #[test]
    #[tag(unit)]
    fn malformed_delimiters_report_the_responsible_source_location() {
        for (text, location, message) in [
            (")", "line 1:1", "unmatched closing parenthesis"),
            ("\n  (atom", "line 2:3", "unterminated list"),
            ("(a \"unfinished", "line 1:4", "unterminated string"),
        ] {
            let error = parse("broken.domain", text).unwrap_err();
            assert_eq!(error.locator.source, "broken.domain");
            assert_eq!(error.locator.declaration, location);
            assert_eq!(error.message, message);
            assert_eq!(error.locator.task, None);
            assert_eq!(error.locator.step, None);
        }
    }

    #[test]
    #[tag(unit)]
    fn strings_keep_comments_and_backslashes_literal_and_columns_count_characters() {
        let forms = parse("unicode", "α (β \"// \\n\")").unwrap();
        let children = forms[1].list().unwrap();
        assert_eq!(forms[1].span.column, 3);
        assert_eq!(children[0].span.column, 4);
        assert_eq!(children[1].kind, Kind::String("// \\n".into()));
    }

    #[test]
    #[tag(unit)]
    fn text_and_source_identity_capacity_are_checked_before_parsing() {
        // These exact sizes are the parser's public admission boundaries.
        assert!(parse("source", &" ".repeat(MAX_BYTES)).unwrap().is_empty());
        assert!(parse("source", &" ".repeat(MAX_BYTES + 1)).is_err());
        assert!(parse(&"s".repeat(MAX_SOURCE), "").is_ok());
        let error = parse(&"s".repeat(MAX_SOURCE + 1), "").unwrap_err();
        assert_eq!(error.locator.source, "<oversized source>");
    }

    #[test]
    #[tag(unit)]
    fn nested_list_capacity_accepts_sixty_four_and_rejects_the_next_level() {
        // The stated nesting bound counts lists, including an empty innermost list.
        let text = |depth| format!("{}{}", "(".repeat(depth), ")".repeat(depth));
        assert!(parse("depth", &text(MAX_DEPTH)).is_ok());
        assert!(
            parse("depth", &text(MAX_DEPTH + 1))
                .unwrap_err()
                .message
                .contains("nesting capacity")
        );
    }

    #[test]
    #[tag(unit)]
    fn node_capacity_counts_atoms_strings_and_lists() {
        // One enclosing list plus each atom uses the exact total-node allowance.
        let mut text = format!("({})", "a ".repeat(MAX_NODES - 1));
        assert!(parse("n", &text).is_ok());
        text.push_str(" \"extra\"");
        assert!(
            parse("n", &text)
                .unwrap_err()
                .message
                .contains("node capacity")
        );
    }
}

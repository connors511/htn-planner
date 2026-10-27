//! Render only a fixed prefix; never clone or traverse an arbitrary Value tree.
use crate::{
    Value,
    compiler::CompiledExpression,
    search::{expressions, state::Bindings},
};
use core::fmt::Write;
pub(in crate::search) struct Rendered {
    pub text: String,
    pub truncated: bool,
}
struct Text {
    value: String,
    limit: usize,
    truncated: bool,
}
impl Write for Text {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        let remaining = self.limit.saturating_sub(self.value.len());
        if value.len() <= remaining {
            self.value.push_str(value);
        } else if remaining >= '…'.len_utf8() {
            self.truncated = true;
            let mut end = remaining - '…'.len_utf8();
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            self.value.push_str(&value[..end]);
            self.value.push('…');
        } else {
            self.truncated = true;
        }
        Ok(())
    }
}
pub(super) fn bounded(value: &str, limit: usize) -> String {
    let mut text = Text {
        value: String::with_capacity(limit),
        limit,
        truncated: false,
    };
    text.write_str(value).unwrap_or_default();
    text.value
}
fn value(text: &mut Text, value: &Value, depth: usize) {
    if text.value.len() + 3 >= text.limit {
        text.truncated = true;
        return;
    }
    match value {
        Value::Bool(v) => {
            write!(text, "{v}").unwrap_or_default();
        }
        Value::Int(v) => {
            write!(text, "{v}").unwrap_or_default();
        }
        Value::Fixed { raw, scale } => {
            write!(text, "{raw}@{scale}").unwrap_or_default();
        }
        Value::Symbol(v) => {
            text.write_str(v).unwrap_or_default();
        }
        Value::Ref { key, .. } => {
            write!(text, "ref:{key}").unwrap_or_default();
        }
        Value::Record { fields, .. } if depth < 2 => {
            text.write_str("{").unwrap_or_default();
            for (i, field) in fields.iter().take(4).enumerate() {
                if i != 0 {
                    text.write_str(",").unwrap_or_default();
                }
                self::value(text, field, depth + 1);
            }
            if fields.len() > 4 {
                text.truncated = true;
                text.write_str("…").unwrap_or_default();
            }
            text.write_str("}").unwrap_or_default();
        }
        Value::Record { .. } => {
            text.truncated = true;
            text.write_str("{…}").unwrap_or_default();
        }
    }
}
pub(in crate::search) fn arguments(args: &[CompiledExpression], env: &Bindings) -> Rendered {
    let mut text = Text {
        value: String::with_capacity(96),
        limit: 96,
        truncated: false,
    };
    for (i, arg) in args.iter().take(6).enumerate() {
        if i != 0 {
            text.write_str(",").unwrap_or_default();
        }
        if let Ok((v, _)) = expressions::value(arg, env) {
            value(&mut text, v, 0);
        } else {
            text.write_str("?").unwrap_or_default();
        }
    }
    if args.len() > 6 {
        text.truncated = true;
        text.write_str("…").unwrap_or_default();
    }
    Rendered {
        text: text.value,
        truncated: text.truncated,
    }
}
pub(in crate::search) fn values(args: &[Value]) -> Rendered {
    let mut text = Text {
        value: String::with_capacity(96),
        limit: 96,
        truncated: false,
    };
    for (i, arg) in args.iter().take(6).enumerate() {
        if i != 0 {
            text.write_str(",").unwrap_or_default();
        }
        value(&mut text, arg, 0);
    }
    if args.len() > 6 {
        text.truncated = true;
        text.write_str("…").unwrap_or_default();
    }
    Rendered {
        text: text.value,
        truncated: text.truncated,
    }
}
pub(super) fn numeric(
    left: &CompiledExpression,
    right: &CompiledExpression,
    env: &Bindings,
) -> Rendered {
    let mut text = Text {
        value: String::with_capacity(96),
        limit: 96,
        truncated: false,
    };
    for (i, arg) in [left, right].into_iter().enumerate() {
        if i != 0 {
            text.write_str(",").unwrap_or_default();
        }
        if let Ok((v, _)) = expressions::value(arg, env) {
            value(&mut text, v, 0);
        } else {
            text.write_str("?").unwrap_or_default();
        }
    }
    Rendered {
        text: text.value,
        truncated: text.truncated,
    }
}

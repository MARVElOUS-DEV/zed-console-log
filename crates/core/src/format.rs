use crate::config::{Quote, Settings};

pub const MARKER: &str = "/* zed-console-log */";

#[derive(Clone, Debug, Default)]
pub struct Context {
    pub class_name: Option<String>,
    pub function_name: Option<String>,
}

pub fn quote_string(value: &str, style: Quote) -> String {
    let delimiter = if style == Quote::Single { '\'' } else { '"' };
    let mut result = String::from(delimiter);
    for c in value.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\r' => result.push_str("\\r"),
            '\n' => result.push_str("\\n"),
            '\u{2028}' => result.push_str("\\u2028"),
            '\u{2029}' => result.push_str("\\u2029"),
            c if c == delimiter => {
                result.push('\\');
                result.push(c);
            }
            c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push(delimiter);
    result
}

pub fn label(
    uri: &str,
    line: usize,
    expression: &str,
    context: &Context,
    settings: &Settings,
) -> String {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let raw_file = path.rsplit('/').next().unwrap_or("untitled");
    let bytes = raw_file.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            decoded.push(value);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let file = String::from_utf8_lossy(&decoded);
    let mut location = Vec::new();
    if settings.include_file_name {
        location.push(file.into_owned());
    }
    if settings.include_line_number {
        location.push(line.to_string());
    }
    let mut parts = vec![settings.prefix.clone(), location.join(":")];
    if settings.include_class_name {
        parts.extend(context.class_name.iter().cloned());
    }
    if settings.include_function_name {
        parts.extend(context.function_name.iter().cloned());
    }
    parts.push(format!("{}:", expression.trim()));
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(&settings.delimiter)
}

pub fn statement(callee: &str, message: &str, expression: &str, settings: &Settings) -> String {
    let args = if callee == "console.table" {
        expression.to_string()
    } else {
        format!("{}, {expression}", quote_string(message, settings.quote))
    };
    format!(
        "{MARKER} {callee}({args}){}",
        if settings.semicolon { ";" } else { "" }
    )
}

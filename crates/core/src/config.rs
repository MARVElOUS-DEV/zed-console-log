use serde::{Deserialize, Serialize};

pub const METHODS: &[&str] = &["log", "info", "debug", "warn", "error", "table"];

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quote {
    #[default]
    Single,
    Double,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InsertPosition {
    Above,
    #[default]
    Below,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Settings {
    pub prefix: String,
    pub delimiter: String,
    pub quote: Quote,
    pub semicolon: bool,
    pub include_file_name: bool,
    pub include_line_number: bool,
    pub include_function_name: bool,
    pub include_class_name: bool,
    pub insert_position: InsertPosition,
    pub methods: Vec<String>,
    pub custom_function: Option<String>,
    pub indent_size: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            prefix: "ZCL".into(),
            delimiter: " ~ ".into(),
            quote: Quote::Single,
            semicolon: true,
            include_file_name: true,
            include_line_number: true,
            include_function_name: true,
            include_class_name: true,
            insert_position: InsertPosition::Below,
            methods: METHODS.iter().map(|s| (*s).into()).collect(),
            custom_function: None,
            indent_size: 2,
        }
    }
}

impl Settings {
    pub fn parse(value: serde_json::Value) -> Result<Self, String> {
        let mut settings: Self = if value.is_null() {
            Self::default()
        } else {
            serde_json::from_value(value).map_err(|error| error.to_string())?
        };
        if [&settings.prefix, &settings.delimiter]
            .iter()
            .any(|value| value.contains(['\r', '\n', '\u{2028}', '\u{2029}']))
        {
            return Err("prefix and delimiter must be single-line strings".into());
        }
        if settings
            .methods
            .iter()
            .any(|method| !METHODS.contains(&method.as_str()))
        {
            return Err("methods must contain only log, info, debug, warn, error, or table".into());
        }
        let mut seen = std::collections::HashSet::new();
        settings
            .methods
            .retain(|method| seen.insert(method.clone()));
        if let Some(name) = &settings.custom_function
            && !dotted_identifier(name)
        {
            return Err("customFunction must be a dotted identifier, e.g. logger.debug".into());
        }
        if !(1..=16).contains(&settings.indent_size) {
            return Err("indentSize must be between 1 and 16".into());
        }
        Ok(settings)
    }
}

/// Deliberately accept a small ASCII subset; configuration never becomes arbitrary source.
pub fn dotted_identifier(value: &str) -> bool {
    let mut segments = value.split('.');
    let Some(root) = segments.next() else {
        return false;
    };
    let valid = |part: &str| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    };
    const RESERVED: &[&str] = &[
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ];
    valid(root) && !RESERVED.contains(&root) && segments.all(valid)
}

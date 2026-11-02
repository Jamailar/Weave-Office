//! A small, table-driven syntax highlighter for the languages people open most.
//!
//! It is a lexer, not a parser: it finds comments, strings, numbers, keywords, types, function
//! calls, keys and tags. That is what editors show in color, and it keeps the app small (no
//! grammar bundles, no regex engine).

use std::ops::Range;

/// What a highlighted run of text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Comment,
    String,
    Number,
    Keyword,
    /// Built-in types, constants and capitalized names.
    Type,
    Function,
    /// Object keys (JSON, YAML, TOML), CSS properties, HTML attributes.
    Key,
    /// HTML/XML tag names, CSS selectors, INI/TOML section headers.
    Tag,
    Punctuation,
    /// Annotations, decorators, preprocessor lines, shell variables.
    Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// C-like and script languages: comments, strings, numbers, identifiers.
    Code,
    Json,
    /// YAML, TOML, INI, properties, .env: line-oriented `key: value` / `key = value`.
    Config,
    Markup,
    Css,
    Plain,
}

/// A language the highlighter knows.
#[derive(Clone, Copy, Debug)]
pub struct Language {
    pub name: &'static str,
    family: Family,
    extensions: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Quote characters that start strings.
    quotes: &'static str,
    /// Triple-quoted strings (Python, Kotlin, Swift, TOML).
    triple_quotes: bool,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    case_insensitive: bool,
    /// `@name` or `#name` prefix that marks annotations/decorators/preprocessor lines.
    meta_prefix: Option<char>,
    /// `$name` variables (shell, PHP, Perl).
    dollar_variables: bool,
    /// Line comment marker used by "toggle comment".
    pub comment: Option<&'static str>,
}

const C_TYPES: &[&str] = &[
    "void", "char", "short", "int", "long", "float", "double", "signed", "unsigned", "bool",
    "size_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t",
    "int64_t", "true", "false", "NULL", "nullptr", "auto", "wchar_t",
];

macro_rules! lang {
    ($name:expr, $family:ident, [$($ext:expr),*] $(, $field:ident : $value:expr)* $(,)?) => {
        Language {
            name: $name,
            family: Family::$family,
            extensions: &[$($ext),*],
            $($field: $value,)*
            ..BASE
        }
    };
}

const BASE: Language = Language {
    name: "",
    family: Family::Plain,
    extensions: &[],
    line_comments: &[],
    block_comment: None,
    quotes: "",
    triple_quotes: false,
    keywords: &[],
    types: &[],
    case_insensitive: false,
    meta_prefix: None,
    dollar_variables: false,
    comment: None,
};

pub const PLAIN: Language = lang!("纯文本", Plain, ["txt", "text", "log"]);

pub static LANGUAGES: &[Language] = &[
    PLAIN,
    lang!("Rust", Code, ["rs"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"", comment: Some("//"),
        meta_prefix: Some('#'),
        keywords: &["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while", "yield", "macro_rules"],
        types: &["bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "true", "false", "None", "Some", "Ok", "Err"]),
    lang!("JavaScript", Code, ["js", "mjs", "cjs", "jsx"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'`", comment: Some("//"), meta_prefix: Some('@'),
        keywords: JS_KEYWORDS, types: JS_TYPES),
    lang!("TypeScript", Code, ["ts", "mts", "cts", "tsx"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'`", comment: Some("//"), meta_prefix: Some('@'),
        keywords: TS_KEYWORDS, types: TS_TYPES),
    lang!("Python", Code, ["py", "pyw", "pyi"],
        line_comments: &["#"], quotes: "\"'", triple_quotes: true, comment: Some("#"), meta_prefix: Some('@'),
        keywords: &["and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match", "case", "self"],
        types: &["True", "False", "None", "int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object", "print", "len", "range"]),
    lang!("Go", Code, ["go"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'`", comment: Some("//"),
        keywords: &["break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var"],
        types: &["bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int", "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "true", "false", "nil", "iota", "any"]),
    lang!("C", Code, ["c", "h"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", comment: Some("//"), meta_prefix: Some('#'),
        keywords: &["break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if", "inline", "register", "restrict", "return", "sizeof", "static", "struct", "switch", "typedef", "union", "volatile", "while"],
        types: C_TYPES),
    lang!("C++", Code, ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "ino"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", comment: Some("//"), meta_prefix: Some('#'),
        keywords: &["alignas", "break", "case", "catch", "class", "const", "constexpr", "continue", "decltype", "default", "delete", "do", "else", "enum", "explicit", "export", "extern", "for", "friend", "goto", "if", "inline", "mutable", "namespace", "new", "noexcept", "operator", "override", "private", "protected", "public", "return", "sizeof", "static", "static_cast", "struct", "switch", "template", "this", "throw", "try", "typedef", "typename", "union", "using", "virtual", "volatile", "while", "co_await", "co_return", "concept", "requires"],
        types: C_TYPES),
    lang!("C#", Code, ["cs"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", comment: Some("//"), meta_prefix: Some('#'),
        keywords: &["abstract", "as", "async", "await", "base", "break", "case", "catch", "class", "const", "continue", "default", "delegate", "do", "else", "enum", "event", "explicit", "extern", "finally", "fixed", "for", "foreach", "get", "goto", "if", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "operator", "out", "override", "params", "private", "protected", "public", "readonly", "record", "ref", "return", "sealed", "set", "sizeof", "static", "struct", "switch", "this", "throw", "try", "typeof", "using", "var", "virtual", "void", "while", "yield"],
        types: &["bool", "byte", "char", "decimal", "double", "float", "int", "long", "object", "sbyte", "short", "string", "uint", "ulong", "ushort", "true", "false", "null"]),
    lang!("Java", Code, ["java"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", comment: Some("//"), meta_prefix: Some('@'),
        keywords: &["abstract", "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "else", "enum", "extends", "final", "finally", "for", "goto", "if", "implements", "import", "instanceof", "interface", "native", "new", "package", "private", "protected", "public", "record", "return", "static", "super", "switch", "synchronized", "this", "throw", "throws", "try", "var", "while", "yield"],
        types: &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "true", "false", "null", "String"]),
    lang!("Kotlin", Code, ["kt", "kts"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"'", triple_quotes: true, comment: Some("//"), meta_prefix: Some('@'),
        keywords: &["as", "break", "class", "continue", "do", "else", "for", "fun", "if", "import", "in", "interface", "is", "object", "package", "return", "super", "this", "throw", "try", "typealias", "val", "var", "when", "while", "data", "sealed", "enum", "override", "private", "public", "internal", "protected", "suspend", "companion", "lateinit", "open"],
        types: &["true", "false", "null", "Int", "Long", "String", "Boolean", "Double", "Float", "Unit", "Any"]),
    lang!("Swift", Code, ["swift"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"", triple_quotes: true, comment: Some("//"), meta_prefix: Some('@'),
        keywords: &["as", "associatedtype", "break", "case", "catch", "class", "continue", "default", "defer", "do", "else", "enum", "extension", "fallthrough", "for", "func", "guard", "if", "import", "in", "init", "inout", "is", "let", "private", "protocol", "public", "repeat", "return", "self", "static", "struct", "subscript", "switch", "throw", "throws", "try", "var", "where", "while", "async", "await"],
        types: &["true", "false", "nil", "Int", "Double", "Float", "String", "Bool", "Character", "Array", "Dictionary", "Set", "Optional", "Void"]),
    lang!("PHP", Code, ["php"],
        line_comments: &["//", "#"], block_comment: Some(("/*", "*/")), quotes: "\"'", comment: Some("//"), dollar_variables: true,
        keywords: &["abstract", "and", "as", "break", "case", "catch", "class", "const", "continue", "declare", "default", "do", "echo", "else", "elseif", "extends", "final", "finally", "fn", "for", "foreach", "function", "global", "if", "implements", "include", "interface", "match", "namespace", "new", "or", "private", "protected", "public", "require", "return", "static", "switch", "throw", "trait", "try", "use", "while", "yield"],
        types: &["true", "false", "null", "TRUE", "FALSE", "NULL", "int", "float", "string", "bool", "array"]),
    lang!("Ruby", Code, ["rb", "rake", "gemspec"],
        line_comments: &["#"], quotes: "\"'", comment: Some("#"),
        keywords: &["alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure", "for", "if", "in", "module", "next", "not", "or", "redo", "rescue", "retry", "return", "self", "super", "then", "undef", "unless", "until", "when", "while", "yield", "require", "attr_accessor"],
        types: &["true", "false", "nil"]),
    lang!("Lua", Code, ["lua"],
        line_comments: &["--"], block_comment: Some(("--[[", "]]")), quotes: "\"'", comment: Some("--"),
        keywords: &["and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in", "local", "not", "or", "repeat", "return", "then", "until", "while"],
        types: &["true", "false", "nil"]),
    lang!("Shell", Code, ["sh", "bash", "zsh", "fish", "command"],
        line_comments: &["#"], quotes: "\"'", comment: Some("#"), dollar_variables: true,
        keywords: &["if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "return", "local", "export", "readonly", "set", "unset", "source", "echo", "exit", "cd", "shift", "trap", "eval", "exec"],
        types: &["true", "false"]),
    lang!("PowerShell", Code, ["ps1", "psm1"],
        line_comments: &["#"], block_comment: Some(("<#", "#>")), quotes: "\"'", comment: Some("#"), dollar_variables: true, case_insensitive: true,
        keywords: &["begin", "break", "catch", "class", "continue", "do", "else", "elseif", "end", "exit", "filter", "finally", "for", "foreach", "function", "if", "in", "param", "process", "return", "switch", "throw", "trap", "try", "until", "while"],
        types: &["$true", "$false", "$null"]),
    lang!("SQL", Code, ["sql"],
        line_comments: &["--"], block_comment: Some(("/*", "*/")), quotes: "'\"`", comment: Some("--"), case_insensitive: true,
        keywords: &["select", "from", "where", "and", "or", "not", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "add", "index", "view", "join", "left", "right", "inner", "outer", "full", "on", "group", "by", "order", "having", "limit", "offset", "as", "distinct", "union", "all", "case", "when", "then", "else", "end", "in", "is", "like", "between", "exists", "primary", "key", "foreign", "references", "default", "unique", "with", "returning", "begin", "commit", "rollback", "asc", "desc"],
        types: &["null", "true", "false", "int", "integer", "bigint", "smallint", "text", "varchar", "char", "boolean", "date", "timestamp", "real", "float", "double", "numeric", "decimal", "blob", "serial"]),
    lang!("Dockerfile", Code, ["dockerfile"],
        line_comments: &["#"], quotes: "\"'", comment: Some("#"), dollar_variables: true, case_insensitive: true,
        keywords: &["from", "run", "cmd", "label", "expose", "env", "add", "copy", "entrypoint", "volume", "user", "workdir", "arg", "onbuild", "stopsignal", "healthcheck", "shell", "as"]),
    lang!("Makefile", Code, ["mk", "makefile"],
        line_comments: &["#"], quotes: "\"'", comment: Some("#"), dollar_variables: true,
        keywords: &["ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "define", "endef", "export", ".PHONY"]),
    lang!("JSON", Json, ["json", "jsonc", "json5", "geojson", "webmanifest", "ipynb"],
        line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: "\"", types: &["true", "false", "null"]),
    lang!("YAML", Config, ["yaml", "yml"], line_comments: &["#"], quotes: "\"'", comment: Some("#"),
        types: &["true", "false", "null", "yes", "no", "on", "off", "~"]),
    lang!("TOML", Config, ["toml", "lock"], line_comments: &["#"], quotes: "\"'", triple_quotes: true, comment: Some("#"),
        types: &["true", "false"]),
    lang!("INI", Config, ["ini", "cfg", "conf", "properties", "env", "editorconfig", "gitconfig", "desktop"],
        line_comments: &["#", ";"], quotes: "\"'", comment: Some("#"), types: &["true", "false", "yes", "no", "on", "off"]),
    lang!("HTML", Markup, ["html", "htm", "xhtml", "vue", "svelte"], comment: None),
    lang!(
        "XML",
        Markup,
        [
            "xml",
            "svg",
            "plist",
            "xsd",
            "xsl",
            "csproj",
            "fsproj",
            "props",
            "targets",
            "storyboard",
            "xaml",
            "rss",
            "atom"
        ]
    ),
    lang!("CSS", Css, ["css", "scss", "sass", "less"], block_comment: Some(("/*", "*/")), line_comments: &[], quotes: "\"'"),
    lang!("Markdown", Plain, ["md", "markdown", "mdown", "mkd"]),
];

const JS_KEYWORDS: &[&str] = &[
    "async",
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
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const JS_TYPES: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "NaN",
    "Infinity",
    "console",
    "window",
    "document",
];
const TS_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "namespace",
    "new",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "satisfies",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "type",
    "typeof",
    "var",
    "void",
    "while",
    "yield",
];
const TS_TYPES: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "string",
    "number",
    "boolean",
    "any",
    "unknown",
    "never",
    "object",
    "symbol",
    "bigint",
    "void",
];

impl Language {
    /// The language for a file name, by extension or well-known name.
    pub fn for_path(name: &str) -> &'static Language {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let lower = file.to_ascii_lowercase();
        let special = match lower.as_str() {
            "dockerfile" | "containerfile" => Some("Dockerfile"),
            "makefile" | "gnumakefile" => Some("Makefile"),
            "cmakelists.txt" => Some("Shell"),
            ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" => Some("Shell"),
            ".gitignore" | ".dockerignore" | ".npmrc" => Some("INI"),
            _ if lower.starts_with(".env") => Some("INI"),
            _ => None,
        };
        if let Some(name) = special {
            return Self::by_name(name).unwrap_or(&PLAIN_REF);
        }
        let ext = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
        LANGUAGES
            .iter()
            .find(|language| language.extensions.contains(&ext))
            .unwrap_or(&PLAIN_REF)
    }

    /// A language by its display name or a fence tag (```js, ```python, ...).
    pub fn by_name(name: &str) -> Option<&'static Language> {
        let lower = name.trim().to_ascii_lowercase();
        let alias = match lower.as_str() {
            "js" | "javascript" | "node" => "javascript",
            "ts" | "typescript" => "typescript",
            "py" | "python3" => "python",
            "rs" => "rust",
            "sh" | "bash" | "zsh" | "shell" | "console" => "shell",
            "c++" | "cpp" | "cxx" => "c++",
            "cs" | "csharp" => "c#",
            "yml" => "yaml",
            "md" => "markdown",
            "golang" => "go",
            "kt" => "kotlin",
            "rb" => "ruby",
            "ps" | "ps1" | "pwsh" => "powershell",
            "jsonc" | "json5" => "json",
            "htm" | "vue" | "svelte" => "html",
            "svg" | "plist" => "xml",
            "scss" | "less" | "sass" => "css",
            "docker" => "dockerfile",
            "make" => "makefile",
            "text" | "txt" | "plain" | "plaintext" => "纯文本",
            other => other,
        };
        LANGUAGES
            .iter()
            .find(|language| language.name.to_lowercase() == alias)
            .or_else(|| {
                LANGUAGES
                    .iter()
                    .find(|language| language.extensions.contains(&alias))
            })
    }

    pub fn is_plain(&self) -> bool {
        self.family == Family::Plain
    }
}

static PLAIN_REF: Language = PLAIN;

/// Highlight `text`; `offset` is added to every range (for code blocks inside a larger text).
pub fn highlight(
    language: &Language,
    text: &str,
    offset: usize,
    out: &mut Vec<(Range<usize>, Token)>,
) {
    match language.family {
        Family::Plain => {}
        Family::Code | Family::Json => code(language, text, offset, out),
        Family::Config => config(language, text, offset, out),
        Family::Markup => markup(text, offset, out),
        Family::Css => css(language, text, offset, out),
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Length in bytes of the number starting at `s` (which starts with a digit or `.digit`).
fn number_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    if s.starts_with("0x") || s.starts_with("0X") || s.starts_with("0b") || s.starts_with("0o") {
        i = 2;
    }
    while i < bytes.len() {
        let b = bytes[i];
        let exponent_sign = (b == b'+' || b == b'-')
            && i > 0
            && matches!(bytes[i - 1], b'e' | b'E')
            && !s.starts_with("0x");
        if b.is_ascii_alphanumeric()
            || b == b'_'
            || b == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            || exponent_sign
        {
            i += 1;
        } else {
            break;
        }
    }
    i.max(1)
}

/// The end of the string starting at `start` (which is at the opening quote).
fn string_end(text: &str, start: usize, quote: &str, multiline: bool, escapes: bool) -> usize {
    let bytes = text.as_bytes();
    let mut i = start + quote.len();
    while i < bytes.len() {
        if escapes && bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(quote.as_bytes()) {
            return i + quote.len();
        }
        if bytes[i] == b'\n' && !multiline {
            return i;
        }
        i += 1;
    }
    text.len()
}

fn code(language: &Language, text: &str, offset: usize, out: &mut Vec<(Range<usize>, Token)>) {
    let json = language.family == Family::Json;
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line_start = true;
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let at_line_start = std::mem::replace(&mut line_start, false);
        if let Some((open, close)) = language.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(text.len(), |e| i + open.len() + e + close.len());
            out.push((offset + i..offset + end, Token::Comment));
            i = end;
            continue;
        }
        if let Some(marker) = language
            .line_comments
            .iter()
            .find(|m| rest.starts_with(**m))
        {
            // `#` starts a comment in scripts, but `#include` / `#[attr]` are meta in C and Rust.
            if !(language.meta_prefix == Some('#') && marker.starts_with('#')) {
                let end = rest.find('\n').map_or(text.len(), |e| i + e);
                out.push((offset + i..offset + end, Token::Comment));
                i = end;
                continue;
            }
        }
        if let Some(prefix) = language.meta_prefix
            && c == prefix
        {
            if prefix == '#' && (at_line_start || rest.starts_with("#[") || rest.starts_with("#!"))
            {
                // A preprocessor line or a Rust attribute: color up to the end of the line or `]`.
                let end = if rest.starts_with("#[") || rest.starts_with("#![") {
                    rest.find(']').map_or(text.len(), |e| i + e + 1)
                } else {
                    rest.find('\n').map_or(text.len(), |e| i + e)
                };
                out.push((offset + i..offset + end, Token::Meta));
                i = end;
                continue;
            }
            if prefix == '@' {
                let end = i
                    + 1
                    + rest[1..]
                        .find(|ch: char| !is_ident(ch) && ch != '.')
                        .unwrap_or(rest.len() - 1);
                out.push((offset + i..offset + end, Token::Meta));
                i = end;
                continue;
            }
        }
        if language.dollar_variables && c == '$' {
            let end = if rest.starts_with("${") {
                rest.find('}').map_or(text.len(), |e| i + e + 1)
            } else {
                i + 1
                    + rest[1..]
                        .find(|ch: char| !is_ident(ch))
                        .unwrap_or(rest.len() - 1)
            };
            if end > i + 1 {
                out.push((offset + i..offset + end, Token::Meta));
                i = end;
                continue;
            }
        }
        if language.quotes.contains(c) {
            let triple: String = std::iter::repeat_n(c, 3).collect();
            let (quote, multiline) = if language.triple_quotes && rest.starts_with(&triple) {
                (triple.as_str(), true)
            } else {
                (&rest[..c.len_utf8()], c == '`')
            };
            // In Rust a single quote is usually a lifetime; only `'x'` / `'\n'` are chars.
            let quote = quote.to_owned();
            let end = string_end(text, i, &quote, multiline, true);
            let token = if json && text[end..].trim_start_matches([' ', '\t']).starts_with(':') {
                Token::Key
            } else {
                Token::String
            };
            out.push((offset + i..offset + end, token));
            i = end;
            continue;
        }
        if c == '\'' && language.name == "Rust" {
            // Lifetime or char literal.
            let close = rest[1..]
                .char_indices()
                .nth(1)
                .filter(|&(_, ch)| ch == '\'');
            let end = if rest[1..].starts_with('\\') {
                rest[2..].find('\'').map_or(i + 1, |e| i + 2 + e + 1)
            } else if let Some((e, _)) = close {
                i + 1 + e + 1
            } else {
                i + 1
                    + rest[1..]
                        .find(|ch: char| !is_ident(ch))
                        .unwrap_or(rest.len() - 1)
            };
            out.push((
                offset + i..offset + end,
                if close.is_some() || rest[1..].starts_with('\\') {
                    Token::String
                } else {
                    Token::Meta
                },
            ));
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let end = i + number_len(rest);
            out.push((offset + i..offset + end, Token::Number));
            i = end;
            continue;
        }
        if c == '-' && json && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let end = i + 1 + number_len(&rest[1..]);
            out.push((offset + i..offset + end, Token::Number));
            i = end;
            continue;
        }
        if is_ident_start(c) {
            let mut end = i + rest.find(|ch: char| !is_ident(ch)).unwrap_or(rest.len());
            // Rust macros: `println!`.
            if language.name == "Rust"
                && text[end..].starts_with('!')
                && !text[end..].starts_with("!=")
            {
                end += 1;
                out.push((offset + i..offset + end, Token::Function));
                i = end;
                continue;
            }
            let word = &text[i..end];
            let matches = |list: &[&str]| {
                if language.case_insensitive {
                    list.iter().any(|k| k.eq_ignore_ascii_case(word))
                } else {
                    list.contains(&word)
                }
            };
            let token = if matches(language.keywords) {
                Some(Token::Keyword)
            } else if matches(language.types) {
                Some(Token::Type)
            } else if text[end..].trim_start_matches(' ').starts_with('(') {
                Some(Token::Function)
            } else if word.chars().next().is_some_and(char::is_uppercase)
                && word.chars().any(char::is_lowercase)
                && !language.case_insensitive
            {
                Some(Token::Type)
            } else {
                None
            };
            if let Some(token) = token {
                out.push((offset + i..offset + end, token));
            }
            i = end;
            continue;
        }
        if "{}[]()".contains(c) {
            out.push((offset + i..offset + i + 1, Token::Punctuation));
        }
        i += c.len_utf8();
    }
}

fn config(language: &Language, text: &str, offset: usize, out: &mut Vec<(Range<usize>, Token)>) {
    let mut start = 0;
    let mut in_triple: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let line_offset = start;
        start += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        if let Some(quote) = in_triple {
            match body.find(quote) {
                Some(end) => {
                    out.push((
                        offset + line_offset..offset + line_offset + end + 3,
                        Token::String,
                    ));
                    in_triple = None;
                }
                None => out.push((
                    offset + line_offset..offset + line_offset + body.len(),
                    Token::String,
                )),
            }
            continue;
        }
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();
        let at = line_offset + indent;
        if trimmed.is_empty() {
            continue;
        }
        if language
            .line_comments
            .iter()
            .any(|m| trimmed.starts_with(m))
        {
            out.push((
                offset + at..offset + line_offset + body.len(),
                Token::Comment,
            ));
            continue;
        }
        if trimmed.starts_with('[') && language.name != "YAML" {
            out.push((offset + at..offset + line_offset + body.len(), Token::Tag));
            continue;
        }
        if language.name == "YAML" && (trimmed == "---" || trimmed == "...") {
            out.push((
                offset + at..offset + line_offset + body.len(),
                Token::Punctuation,
            ));
            continue;
        }
        // The key: up to `:` (YAML) or `=` (TOML/INI), outside quotes.
        let mut value_start = indent;
        let list_item = language.name == "YAML" && trimmed.starts_with("- ");
        let key_from = if list_item { indent + 2 } else { indent };
        let separator = if language.name == "YAML" { ": " } else { "=" };
        let key_part = &body[key_from..];
        let split = if language.name == "YAML" {
            key_part
                .find(separator)
                .or_else(|| key_part.ends_with(':').then(|| key_part.len() - 1))
        } else {
            key_part.find(separator).or_else(|| {
                (language.name == "INI")
                    .then(|| key_part.find(':'))
                    .flatten()
            })
        };
        if list_item {
            out.push((offset + at..offset + at + 1, Token::Punctuation));
            value_start = indent + 2;
        }
        if let Some(split) = split {
            let key = key_part[..split].trim();
            let quoted = key.len() >= 2
                && ((key.starts_with('"') && key.ends_with('"'))
                    || (key.starts_with('\'') && key.ends_with('\'')));
            if quoted || !key.contains(['"', '\'', '{', '[']) {
                let key_end = key_from + key_part[..split].trim_end().len();
                out.push((
                    offset + line_offset + key_from..offset + line_offset + key_end,
                    Token::Key,
                ));
                value_start = key_from + split + 1;
            }
        }
        // The value.
        let value = &body[value_start..];
        let trimmed_value = value.trim();
        let value_at = line_offset + value_start + (value.len() - value.trim_start().len());
        if trimmed_value.is_empty() {
            continue;
        }
        let first = trimmed_value.chars().next().unwrap_or(' ');
        if language.triple_quotes
            && (trimmed_value.starts_with("\"\"\"") || trimmed_value.starts_with("'''"))
        {
            let quote = &trimmed_value[..3];
            if let Some(end) = trimmed_value[3..].find(quote) {
                out.push((
                    offset + value_at..offset + value_at + 3 + end + 3,
                    Token::String,
                ));
            } else {
                in_triple = Some(if quote == "\"\"\"" { "\"\"\"" } else { "'''" });
                out.push((
                    offset + value_at..offset + line_offset + body.len(),
                    Token::String,
                ));
            }
            continue;
        }
        if language.quotes.contains(first) {
            let end = string_end(
                &text[..line_offset + body.len()],
                value_at,
                &first.to_string(),
                false,
                first == '"',
            );
            out.push((offset + value_at..offset + end, Token::String));
            continue;
        }
        // Trailing comment.
        let comment_at = language
            .line_comments
            .iter()
            .filter_map(|m| trimmed_value.find(&format!(" {m}")))
            .min();
        let scalar = comment_at.map_or(trimmed_value, |c| trimmed_value[..c].trim_end());
        let token = if language
            .types
            .iter()
            .any(|t| t.eq_ignore_ascii_case(scalar))
        {
            Some(Token::Type)
        } else if !scalar.is_empty()
            && scalar
                .trim_start_matches(['-', '+'])
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
            && scalar
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-+._:".contains(c))
        {
            Some(Token::Number)
        } else if scalar.starts_with(['&', '*', '!']) && language.name == "YAML" {
            Some(Token::Meta)
        } else if "[{|>".contains(first) {
            None
        } else {
            Some(Token::String)
        };
        if let Some(token) = token {
            out.push((offset + value_at..offset + value_at + scalar.len(), token));
        }
        if let Some(c) = comment_at {
            out.push((
                offset + value_at + c + 1..offset + line_offset + body.len(),
                Token::Comment,
            ));
        }
    }
}

fn markup(text: &str, offset: usize, out: &mut Vec<(Range<usize>, Token)>) {
    let mut i = 0;
    while let Some(found) = text[i..].find('<') {
        let start = i + found;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(text.len(), |e| start + e + 3);
            out.push((offset + start..offset + end, Token::Comment));
            i = end;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map_or(text.len(), |e| start + e + 3);
            out.push((offset + start..offset + end, Token::String));
            i = end;
            continue;
        }
        let name_from = start + 1 + usize::from(rest[1..].starts_with(['/', '?', '!']));
        let name_len = text[name_from..]
            .find(|c: char| !(c.is_alphanumeric() || "-_:.".contains(c)))
            .unwrap_or(text.len() - name_from);
        if name_len == 0 {
            i = start + 1;
            continue;
        }
        out.push((offset + start..offset + name_from, Token::Punctuation));
        out.push((
            offset + name_from..offset + name_from + name_len,
            Token::Tag,
        ));
        // Attributes until `>`.
        let mut j = name_from + name_len;
        let bytes = text.as_bytes();
        while j < text.len() {
            let c = bytes[j];
            if c == b'>' {
                out.push((offset + j..offset + j + 1, Token::Punctuation));
                j += 1;
                break;
            }
            if c == b'"' || c == b'\'' {
                let end = string_end(text, j, if c == b'"' { "\"" } else { "'" }, true, false);
                out.push((offset + j..offset + end, Token::String));
                j = end;
                continue;
            }
            if (c as char).is_ascii_alphabetic() {
                let len = text[j..]
                    .find(|ch: char| !(ch.is_alphanumeric() || "-_:@.".contains(ch)))
                    .unwrap_or(text.len() - j);
                out.push((offset + j..offset + j + len, Token::Key));
                j += len;
                continue;
            }
            j += 1;
        }
        // <script> and <style> bodies are left uncolored rather than mis-lexed as markup.
        let name = &text[name_from..name_from + name_len];
        if !rest.starts_with("</")
            && (name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style"))
        {
            let close = format!("</{name}");
            let end = text[j..].find(&close).map_or(text.len(), |e| j + e);
            let inner = Language::by_name(if name.eq_ignore_ascii_case("script") {
                "javascript"
            } else {
                "css"
            });
            if let Some(inner) = inner {
                highlight(inner, &text[j..end], offset + j, out);
            }
            j = end;
        }
        i = j.max(start + 1);
    }
}

fn css(language: &Language, text: &str, offset: usize, out: &mut Vec<(Range<usize>, Token)>) {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with("/*") {
            let end = rest.find("*/").map_or(text.len(), |e| i + e + 2);
            out.push((offset + i..offset + end, Token::Comment));
            i = end;
            continue;
        }
        if rest.starts_with("//") && language.extensions.contains(&"scss") {
            let end = rest.find('\n').map_or(text.len(), |e| i + e);
            out.push((offset + i..offset + end, Token::Comment));
            i = end;
            continue;
        }
        let c = bytes[i];
        match c {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'"' | b'\'' => {
                let end = string_end(text, i, if c == b'"' { "\"" } else { "'" }, false, true);
                out.push((offset + i..offset + end, Token::String));
                i = end;
            }
            b'@' => {
                let len = 1 + rest[1..]
                    .find(|ch: char| !(ch.is_alphanumeric() || ch == '-'))
                    .unwrap_or(rest.len() - 1);
                out.push((offset + i..offset + i + len, Token::Keyword));
                i += len;
            }
            _ if depth == 0 => {
                // A selector up to `{`.
                let end = rest.find(['{', ';']).map_or(text.len(), |e| i + e);
                let selector = text[i..end].trim_end();
                if !selector.trim().is_empty() {
                    out.push((offset + i..offset + i + selector.len(), Token::Tag));
                }
                i = end.max(i + 1);
            }
            _ if (c as char).is_ascii_alphabetic() || c == b'-' || c == b'$' => {
                let len = rest
                    .find(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '$'))
                    .unwrap_or(rest.len());
                let after = text[i + len..].trim_start();
                if after.starts_with(':') && !after.starts_with("::") {
                    out.push((offset + i..offset + i + len, Token::Key));
                } else if after.starts_with('(') {
                    out.push((offset + i..offset + i + len, Token::Function));
                }
                i += len.max(1);
            }
            b'#' => {
                let len = 1 + rest[1..]
                    .find(|ch: char| !ch.is_ascii_alphanumeric())
                    .unwrap_or(rest.len() - 1);
                out.push((offset + i..offset + i + len, Token::Number));
                i += len;
            }
            _ if c.is_ascii_digit()
                || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let len = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '%'))
                    .unwrap_or(rest.len());
                out.push((offset + i..offset + i + len, Token::Number));
                i += len.max(1);
            }
            _ => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(language: &str, text: &str) -> Vec<(String, Token)> {
        let language = Language::by_name(language).unwrap();
        let mut out = Vec::new();
        highlight(language, text, 0, &mut out);
        out.into_iter()
            .map(|(r, t)| (text[r].to_owned(), t))
            .collect()
    }

    fn has(tokens: &[(String, Token)], text: &str, token: Token) -> bool {
        tokens.iter().any(|(t, k)| t == text && *k == token)
    }

    #[test]
    fn picks_languages_by_extension_and_fence_name() {
        assert_eq!(Language::for_path("src/main.rs").name, "Rust");
        assert_eq!(Language::for_path("A.JSON").name, "JSON");
        assert_eq!(Language::for_path("Dockerfile").name, "Dockerfile");
        assert_eq!(Language::for_path(".env.local").name, "INI");
        assert_eq!(Language::for_path("notes.unknown").name, PLAIN.name);
        assert_eq!(Language::by_name("py").unwrap().name, "Python");
        assert_eq!(Language::by_name("ts").unwrap().name, "TypeScript");
        assert_eq!(Language::by_name("yml").unwrap().name, "YAML");
    }

    #[test]
    fn code_tokens() {
        let t = tokens(
            "rust",
            "// hi\nfn main() { let s = \"x\"; println!(\"{}\", 42); } #[derive(Debug)]",
        );
        assert!(has(&t, "// hi", Token::Comment));
        assert!(has(&t, "fn", Token::Keyword));
        assert!(has(&t, "main", Token::Function));
        assert!(has(&t, "\"x\"", Token::String));
        assert!(has(&t, "println!", Token::Function));
        assert!(has(&t, "42", Token::Number));
        assert!(has(&t, "#[derive(Debug)]", Token::Meta));
        let t = tokens(
            "python",
            "def f():\n    '''doc\n    more'''\n    return None  # done",
        );
        assert!(has(&t, "'''doc\n    more'''", Token::String));
        assert!(has(&t, "None", Token::Type));
        assert!(has(&t, "# done", Token::Comment));
        let t = tokens("sql", "SELECT id FROM t -- c");
        assert!(has(&t, "SELECT", Token::Keyword) && has(&t, "-- c", Token::Comment));
    }

    #[test]
    fn json_keys_and_values() {
        let t = tokens("json", "{\"name\": \"weave\", \"n\": -1.5e3, \"ok\": true}");
        assert!(has(&t, "\"name\"", Token::Key));
        assert!(has(&t, "\"weave\"", Token::String));
        assert!(has(&t, "-1.5e3", Token::Number));
        assert!(has(&t, "true", Token::Type));
    }

    #[test]
    fn config_files() {
        let t = tokens("yaml", "# c\nname: weave\nlist:\n  - item\ncount: 3 # n\n");
        assert!(has(&t, "# c", Token::Comment));
        assert!(has(&t, "name", Token::Key));
        assert!(has(&t, "weave", Token::String));
        assert!(has(&t, "list", Token::Key));
        assert!(has(&t, "3", Token::Number));
        assert!(has(&t, "# n", Token::Comment));
        let t = tokens("toml", "[package]\nname = \"weave\"\nedition = 2024\n");
        assert!(has(&t, "[package]", Token::Tag));
        assert!(has(&t, "name", Token::Key));
        assert!(has(&t, "\"weave\"", Token::String));
        assert!(has(&t, "2024", Token::Number));
    }

    #[test]
    fn markup_and_css() {
        let t = tokens(
            "html",
            "<!-- c --><a href=\"x\">t</a><style>p { color: red; }</style>",
        );
        assert!(has(&t, "<!-- c -->", Token::Comment));
        assert!(has(&t, "a", Token::Tag));
        assert!(has(&t, "href", Token::Key));
        assert!(has(&t, "\"x\"", Token::String));
        assert!(has(&t, "color", Token::Key));
        let t = tokens("css", ".a > b { margin: 0 4px; color: #fff; }");
        assert!(has(&t, ".a > b", Token::Tag));
        assert!(has(&t, "4px", Token::Number));
        assert!(has(&t, "#fff", Token::Number));
    }

    #[test]
    fn never_panics_on_odd_input() {
        for language in LANGUAGES {
            for text in [
                "\"", "'", "/*", "<", "<a b=\"", "#", "$", "${", "0x", "'a", "é\"中", "[",
                "key: \"", "'''",
            ] {
                let mut out = Vec::new();
                highlight(language, text, 0, &mut out);
                for (range, _) in out {
                    assert!(
                        range.start <= range.end && range.end <= text.len(),
                        "{} {text:?}",
                        language.name
                    );
                    assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
                }
            }
        }
    }
}

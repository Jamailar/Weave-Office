//! The one error shape every command reports: a machine-readable code, a message, and where
//! possible a suggestion an agent can act on.

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct CliError {
    /// `usage`, `not_found`, `unsupported_format`, `unknown_command`, `invalid_params`,
    /// `invalid_path`, `password_required`, `open_failed`, `io`, `engine`.
    pub code: &'static str,
    pub message: String,
    pub suggestion: Option<String>,
    /// Valid alternatives (closest command ids, valid ranges), when known.
    pub candidates: Vec<String>,
    /// Extra structured detail (a failed batch's per-command results).
    pub details: Option<Value>,
}

impl CliError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            suggestion: None,
            candidates: Vec::new(),
            details: None,
        }
    }
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("usage", message).suggest("run `weave-office help` for usage")
    }
    pub fn suggest(mut self, s: impl Into<String>) -> Self {
        self.suggestion = Some(s.into());
        self
    }
    pub fn with_candidates(mut self, c: Vec<String>) -> Self {
        self.candidates = c;
        self
    }
    /// Exit status: 2 for command-line mistakes, 1 for everything else.
    pub fn exit_code(&self) -> i32 {
        if self.code == "usage" { 2 } else { 1 }
    }
    pub fn to_json(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message});
        if let Some(s) = &self.suggestion {
            e["suggestion"] = json!(s);
        }
        if !self.candidates.is_empty() {
            e["candidates"] = json!(self.candidates);
        }
        if let Some(d) = &self.details {
            e["details"] = d.clone();
        }
        e
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.message)?;
        if let Some(s) = &self.suggestion {
            write!(f, "\nhint: {s}")?;
        }
        Ok(())
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::new("io", e.to_string())
    }
}

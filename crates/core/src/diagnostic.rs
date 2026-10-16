//! Structured per-file diagnostics shared by every pipeline stage.

use std::fmt;

/// The pipeline stage a diagnostic came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Config,
    Parse,
    LintFix,
    Format,
    CustomFix,
    Convergence,
}

impl Stage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Parse => "parse",
            Self::LintFix => "lint-fix",
            Self::Format => "format",
            Self::CustomFix => "custom-fix",
            Self::Convergence => "convergence",
        }
    }
}

/// One failure attached to a file and stage, with a stable machine-readable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub stage: Stage,
    pub code: String,
    pub message: String,
    /// One-based line in the authored source, when the failure carries a position.
    pub line: Option<u32>,
    /// One-based character column in the authored source.
    pub column: Option<u32>,
}

impl Diagnostic {
    pub fn new(
        path: &str,
        stage: Stage,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.to_string(),
            stage,
            code: code.into(),
            message: message.into(),
            line: None,
            column: None,
        }
    }

    /// Attaches the one-based line/column of `offset` (a UTF-8 byte offset into `source`).
    #[must_use]
    pub fn at(mut self, source: &str, offset: u32) -> Self {
        let (line, column) = line_column(source, offset);
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

/// One-based (line, character column) of a UTF-8 byte offset.
#[must_use]
pub fn line_column(source: &str, offset: u32) -> (u32, u32) {
    let offset = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    let mut line = 1u32;
    let mut column = 1u32;
    for (index, ch) in source.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// A failure that stops construction or a per-file pipeline run.
#[derive(Debug)]
pub enum EngineError {
    /// Configuration could not be compiled or the engine session could not be built.
    Config(String),
    /// One file could not be transformed; carries its structured diagnostics.
    File(Vec<Diagnostic>),
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => formatter.write_str(message),
            Self::File(diagnostics) => {
                for diagnostic in diagnostics {
                    writeln!(
                        formatter,
                        "{}: {} [{}] {}",
                        diagnostic.path,
                        diagnostic.stage.as_str(),
                        diagnostic.code,
                        diagnostic.message
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EngineError {}

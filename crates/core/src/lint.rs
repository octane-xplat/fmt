//! The lint-fix stage: collect validated safe edits through
//! `LintSession::code_actions`, apply the non-overlapping subset, and repeat
//! until the phase settles. No upstream filesystem-fixing entry point is
//! used; this stage never writes.

use std::path::Path;

use tsrx_lint::{LintError, LintSession};

use crate::{
    diagnostic::{Diagnostic, Stage},
    edits::{Edit, apply_edits},
};

/// Recompute cap for the lint-fix phase. Fix application is monotone under a
/// bounded rule set, so hitting this means something upstream is oscillating.
const MAX_LINT_PASSES: u32 = 10;

/// Runs the lint-fix phase over `source`, returning the fixed text.
///
/// `code_actions` returns each fix already validated by an upstream reparse
/// in authored coordinates; this stage selects a deterministic non-overlapping
/// subset and reapplies until no fix survives or the bound is hit.
pub fn lint_fix(
    session: &LintSession,
    path: &Path,
    source: &str,
) -> Result<String, Vec<Diagnostic>> {
    let display = path.display().to_string();
    let mut current = source.to_string();
    for _ in 0..MAX_LINT_PASSES {
        let actions = session
            .code_actions(path, &current)
            .map_err(|error| vec![lint_error_diagnostic(&display, &current, &error)])?;
        if actions.is_empty() {
            return Ok(current);
        }
        let edits: Vec<Edit> = actions
            .iter()
            .map(|fix| Edit {
                start: fix.offset,
                end: fix.offset.saturating_add(fix.length),
                replacement: fix.replacement.clone(),
            })
            .collect();
        if apply_edits(&mut current, &edits) == 0 {
            // Every surviving fix overlapped or fell out of bounds; applying
            // nothing further is the deterministic stop.
            return Ok(current);
        }
    }
    Ok(current)
}

fn lint_error_diagnostic(path: &str, source: &str, error: &LintError) -> Diagnostic {
    let (stage, code, offset) = match error {
        LintError::Projection(error) => (Stage::Parse, "TSRX_PROJECTION", projection_offset(error)),
        LintError::SourceKind(_) => (Stage::Parse, "UNSUPPORTED_EXTENSION", None),
        LintError::Config(_) => (Stage::Config, "OXC_CONFIG", None),
        LintError::Unparsed(unparsed) => {
            let offset = unparsed
                .diagnostics
                .iter()
                .flat_map(|diagnostic| diagnostic.labels.iter())
                .map(|label| label.offset)
                .next();
            (Stage::Parse, "OXC_PARSE", offset)
        }
        _ => (Stage::LintFix, "LINT_FIX", None),
    };
    let diagnostic = Diagnostic::new(path, stage, code, error.to_string());
    match offset {
        Some(offset) => diagnostic.at(source, offset),
        None => diagnostic,
    }
}

fn projection_offset(error: &tsrx_syntax::ProjectionError) -> Option<u32> {
    use tsrx_syntax::ProjectionError as Error;
    match error {
        Error::SourceChanged { offset }
        | Error::UnsupportedSyntax { offset, .. }
        | Error::UnterminatedSyntax { offset, .. }
        | Error::MalformedSyntax { offset, .. }
        | Error::UnclosedTag { offset, .. } => Some(*offset),
        _ => None,
    }
}

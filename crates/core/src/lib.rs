//! Octane Xplat formatting engine: selected safe Oxlint fixes, canonical
//! Oxfmt, and the custom statement-spacing fix — one in-process pipeline over
//! the pinned `tsrx-org/oxc` toolchain for JS/TS/JSX/TSX and TSRX source.
//!
//! Per file, owned buffers move through:
//!
//! 1. [`lint::lint_fix`] — validated safe fixes from `code_actions`, applied
//!    non-overlapping until the phase settles (bounded).
//! 2. [`format::format_source`] — canonical Oxfmt; for `.tsrx`, the upstream
//!    scan → project → format → lift path, settled like upstream.
//! 3. The spacing pass — statement containers collected by [`tree`], gaps
//!    measured and edits applied by [`spacing`].
//! 4. An outer bounded convergence loop over 1–3; on oscillation or
//!    exhaustion the run returns a `convergence` diagnostic and publishes no
//!    partial output.

mod config;
mod diagnostic;
mod edits;
mod format;
mod lint;
mod spacing;
mod tree;
mod utf16;

use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

pub use config::{BLESSED_FORMAT, RULE_REGISTRY, RuleSpec};
pub use diagnostic::{Diagnostic, EngineError, Stage, line_column};
use edits::Insertion;

/// Construction-time options. Configuration resolution is
/// blessed defaults → project files → `format_options`, in that order.
#[derive(Debug, Default)]
pub struct EngineConfig {
    pub cwd: PathBuf,
    /// Explicit `.oxlintrc` path; otherwise discovered upward from `cwd`.
    pub lint_config_path: Option<PathBuf>,
    /// Explicit `.oxfmtrc` path; otherwise discovered upward from `cwd`.
    pub format_config_path: Option<PathBuf>,
    /// Optional camelCase `.oxfmtrc`-shaped option object applied last.
    pub format_options: Option<serde_json::Value>,
}

/// One configured formatter, reusable across files.
pub struct Engine {
    cwd: PathBuf,
    lint: tsrx_lint::LintSession,
    format: config::FormatConfig,
}

/// The result of one successful pipeline run.
#[derive(Debug)]
pub struct FormatOutcome {
    pub code: String,
    /// Whether the final output differs from the input.
    pub changed: bool,
}

impl Engine {
    /// Compiles the lint session and resolves the format configuration.
    ///
    /// # Errors
    ///
    /// Returns `EngineError::Config` for invalid or unsupported configuration.
    pub fn new(config: &EngineConfig) -> Result<Self, EngineError> {
        let lint_source =
            config::build_lint_config_source(&config.cwd, config.lint_config_path.as_deref())?;
        let lint = tsrx_lint::LintSession::new_with_config_source(
            &config.cwd,
            Some(&lint_source),
            &[],
            true,
        )
        .map_err(|error| EngineError::Config(error.to_string()))?;
        let format = config::FormatConfig::new(
            &config.cwd,
            config.format_config_path.as_deref(),
            config.format_options.as_ref(),
        )?;
        Ok(Self {
            cwd: config.cwd.clone(),
            lint,
            format,
        })
    }

    /// Whether `path` is outside this engine's coverage: a formatter ignore
    /// pattern or a lint `ignorePatterns` entry.
    #[must_use]
    pub fn should_ignore(&self, path: &Path) -> bool {
        self.lint.should_ignore(path) || self.format.should_ignore(&self.cwd, path)
    }

    /// Runs the full pipeline over caller-owned text.
    ///
    /// # Errors
    ///
    /// Returns the stage's diagnostics; nothing is written and no partial
    /// output is produced.
    pub fn format_text(&self, path: &Path, source: &str) -> Result<FormatOutcome, Vec<Diagnostic>> {
        let mut current = source.to_string();
        let mut seen: HashSet<u64> = HashSet::new();
        seen.insert(hash(&current));
        for _ in 0..MAX_PIPELINE_PASSES {
            let next = self.once(path, &current)?;
            if next == current {
                let changed = next != source;
                return Ok(FormatOutcome {
                    code: next,
                    changed,
                });
            }
            if !seen.insert(hash(&next)) {
                return Err(vec![Diagnostic::new(
                    &path.display().to_string(),
                    Stage::Convergence,
                    "OSCILLATION",
                    "the format pipeline oscillated between outputs",
                )]);
            }
            current = next;
        }
        Err(vec![Diagnostic::new(
            &path.display().to_string(),
            Stage::Convergence,
            "PASS_LIMIT",
            format!("the format pipeline did not settle within {MAX_PIPELINE_PASSES} passes"),
        )])
    }

    /// One complete pass: lint fixes, then the formatter, then spacing.
    fn once(&self, path: &Path, source: &str) -> Result<String, Vec<Diagnostic>> {
        let fixed = lint::lint_fix(&self.lint, path, source)?;
        let options = self.format.options_for(&self.cwd, path);
        let formatted = format::format_source(path, &fixed, &options)?;
        let containers = tree::collect_containers(path, &formatted)?;
        if std::env::var_os("FMT_DEBUG").is_some() {
            eprintln!("FORMATTED:\n{formatted}\nCONTAINERS: {containers:?}");
        }
        let violations = spacing::filter_disabled(
            &formatted,
            spacing::find_spacing_violations(&formatted, &containers),
        );
        let mut output = formatted;
        edits::apply_insertions(
            &mut output,
            &violations
                .iter()
                .map(|violation| Insertion {
                    position: violation.position,
                    text: violation.text.clone(),
                })
                .collect::<Vec<_>>(),
        );
        Ok(output)
    }
}

/// Outer convergence bound. The lint phase has its own inner bound; four full
/// iterations is generous for the lint→format→spacing feedback.
const MAX_PIPELINE_PASSES: u32 = 4;

fn hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

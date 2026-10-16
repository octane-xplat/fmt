//! Node-API binding over `octane-fmt-core`. Kept deliberately thin: options in,
//! plain-object results out; all policy lives in the core crate.

// napi exposes owned `String` arguments for JS strings; by-value parameters are
// forced by the macro layer.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use napi::bindgen_prelude::{AsyncTask, Task};
use napi::{Env, Error, Result};
use napi_derive::napi;
use octane_fmt_core::{Diagnostic, Engine, EngineConfig, FormatOutcome};

#[napi(object)]
#[derive(Default)]
pub struct FormatterOptions {
    pub cwd: String,
    #[napi(js_name = "lintConfigPath")]
    pub lint_config_path: Option<String>,
    #[napi(js_name = "formatConfigPath")]
    pub format_config_path: Option<String>,
    /// CamelCase `.oxfmtrc`-shaped options applied above file configuration.
    #[napi(js_name = "formatOptions")]
    pub format_options: Option<serde_json::Value>,
}

#[napi(object)]
pub struct FormatInput {
    pub path: String,
    pub source: String,
}

#[napi(object)]
pub struct DiagnosticObject {
    pub path: String,
    /// `config` | `parse` | `lint-fix` | `format` | `custom-fix` | `convergence`
    pub stage: String,
    /// Stable machine-readable code, e.g. `OXC_PARSE`, `TSRX_PROJECTION`.
    pub code: String,
    pub message: String,
    /// One-based line in the authored source.
    pub line: Option<u32>,
    /// One-based character column.
    pub column: Option<u32>,
}

/// `{ ok: true, code, changed }` on success and `{ ok: false, diagnostics }`
/// on failure; the TypeScript surface narrows this into the documented union.
#[napi(object)]
pub struct FormatResultObject {
    pub ok: bool,
    pub code: Option<String>,
    pub changed: Option<bool>,
    pub diagnostics: Option<Vec<DiagnosticObject>>,
}

/// One configured formatter. `createFormatter` throws on invalid or
/// unsupported configuration before any source is parsed.
#[napi(js_name = "Formatter")]
pub struct JsFormatter {
    engine: Arc<Engine>,
}

/// # Errors
///
/// Throws a JS `Error` when the resolved configuration is invalid or
/// unsupported (bad `.oxlintrc`/`.oxfmtrc`, JS config modules, unknown
/// options).
#[napi(js_name = "createFormatter")]
pub fn create_formatter(options: FormatterOptions) -> Result<JsFormatter> {
    let config = EngineConfig {
        cwd: PathBuf::from(&options.cwd),
        lint_config_path: options.lint_config_path.map(PathBuf::from),
        format_config_path: options.format_config_path.map(PathBuf::from),
        format_options: options.format_options,
    };
    let engine = Engine::new(&config).map_err(|error| Error::from_reason(error.to_string()))?;
    Ok(JsFormatter {
        engine: Arc::new(engine),
    })
}

#[napi]
impl JsFormatter {
    /// Whether the engine's ignore sets cover `path`.
    #[napi(js_name = "shouldIgnore")]
    #[must_use]
    pub fn should_ignore(&self, path: String) -> bool {
        self.engine.should_ignore(Path::new(&path))
    }

    #[napi(js_name = "formatText")]
    #[must_use]
    pub fn format_text(&self, input: FormatInput) -> AsyncTask<FormatTask> {
        AsyncTask::new(FormatTask {
            engine: Arc::clone(&self.engine),
            path: input.path,
            source: input.source,
        })
    }
}

pub struct FormatTask {
    engine: Arc<Engine>,
    path: String,
    source: String,
}

impl Task for FormatTask {
    type Output = FormatResultObject;
    type JsValue = FormatResultObject;

    fn compute(&mut self) -> Result<Self::Output> {
        Ok(
            match self.engine.format_text(Path::new(&self.path), &self.source) {
                Ok(FormatOutcome { code, changed }) => FormatResultObject {
                    ok: true,
                    code: Some(code),
                    changed: Some(changed),
                    diagnostics: None,
                },
                Err(diagnostics) => FormatResultObject {
                    ok: false,
                    code: None,
                    changed: None,
                    diagnostics: Some(diagnostics.iter().map(diagnostic_object).collect()),
                },
            },
        )
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

fn diagnostic_object(diagnostic: &Diagnostic) -> DiagnosticObject {
    DiagnosticObject {
        path: diagnostic.path.clone(),
        stage: diagnostic.stage.as_str().to_string(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        line: diagnostic.line,
        column: diagnostic.column,
    }
}

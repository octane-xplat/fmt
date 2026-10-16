//! Configuration resolution: blessed defaults, then project overrides, then
//! explicit caller options.
//!
//! The lint side keeps one explicit rule registry: the in-memory `Oxlintrc`
//! handed to `LintSession` contains only registry rules, so Oxlint's other
//! default rule groups and JavaScript plugins never reach the formatter. A
//! project `.oxlintrc.json` may reconfigure or disable registry rules through
//! `rules` and per-file `overrides`; `env`, `globals`, and `ignorePatterns`
//! pass through unchanged. Everything else — `extends`, plugins, categories,
//! non-registry rules — is rejected or ignored per the documented policy in
//! this crate's README/PLAN.
//!
//! The format side resolves `.oxfmtrc.json(c)` itself because the upstream
//! `FormatSession` cannot accept in-memory options: blessed defaults must sit
//! underneath a project configuration, which its path-based constructors
//! cannot express. The merge order below mirrors upstream
//! `FormatSession::options_for`, with the same refusals (JS config modules,
//! `.editorconfig`, unknown options, embedded-language formatting).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use oxc_adapter::FormatOptions;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::diagnostic::EngineError;

/// The versioned, explicit fixable-rule preset this formatter applies.
///
/// `default` is the severity the rule gets when the project configuration
/// does not mention it. `"off"` entries are supported but dormant until a
/// project enables them, which is how `no-var` ships: upstream-verified as a
/// safe fixer, not part of the blessed default set.
pub const RULE_REGISTRY: &[RuleSpec] = &[
    RuleSpec {
        name: "curly",
        default: "error",
        docs: "require braces around all statements",
    },
    RuleSpec {
        name: "no-var",
        default: "off",
        docs: "rewrite `var` as `let`/`const`; upstream safe fix, off by default",
    },
];

/// One formatter-supported lint rule.
pub struct RuleSpec {
    pub name: &'static str,
    /// Severity applied when the project does not configure the rule.
    pub default: &'static str,
    pub docs: &'static str,
}

/// Blessed formatting defaults: the Octane Xplat style.
pub const BLESSED_FORMAT: &[(&str, Value)] = &[
    ("semi", Value::Bool(false)),
    ("useTabs", Value::Bool(true)),
    ("singleQuote", Value::Bool(true)),
];

const OXFMT_CONFIG_NAMES: &[&str] = &[".oxfmtrc.json", ".oxfmtrc.jsonc"];
const OXLINT_CONFIG_NAMES: &[&str] = &[".oxlintrc.json", ".oxlintrc.jsonc"];
const JS_CONFIG_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "ts", "mts", "cts"];

// ---------------------------------------------------------------------------
// Lint configuration
// ---------------------------------------------------------------------------

/// Builds the in-memory Oxlint configuration JSON for `cwd`.
///
/// Blessed preset first; a discovered or explicit project `.oxlintrc` then
/// overrides registry rules only. Returns `Err` for unreadable/invalid project
/// configuration and for capabilities the formatter cannot honor (`extends`,
/// JS config modules).
pub fn build_lint_config_source(
    cwd: &Path,
    explicit_path: Option<&Path>,
) -> Result<String, EngineError> {
    let mut rules = Map::new();
    for spec in RULE_REGISTRY {
        if spec.default != "off" {
            rules.insert(
                spec.name.to_string(),
                Value::String(spec.default.to_string()),
            );
        }
    }

    let mut merged = Map::new();
    let config_path = explicit_path
        .map(|path| absolutize(cwd, path))
        .or_else(|| discover_upward(cwd, OXLINT_CONFIG_NAMES));

    if let Some(path) = config_path {
        if is_js_config(&path) {
            return Err(EngineError::Config(format!(
                "JavaScript/TypeScript Oxlint config modules are not supported: {}",
                path.display()
            )));
        }
        let source = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!("Unable to read {}: {error}", path.display()))
        })?;
        let project = parse_jsonc(&source).map_err(|detail| {
            EngineError::Config(format!("Invalid {}: {detail}", path.display()))
        })?;
        let project = project.as_object().ok_or_else(|| {
            EngineError::Config(format!("{} must contain a JSON object", path.display()))
        })?;

        if project.contains_key("extends") {
            return Err(EngineError::Config(format!(
                "{}: `extends` is not supported by the formatter; inline the registry-rule \
                 settings instead",
                path.display()
            )));
        }

        if let Some(Value::Object(project_rules)) = project.get("rules") {
            for spec in RULE_REGISTRY {
                if let Some(value) = project_rules.get(spec.name) {
                    rules.insert(spec.name.to_string(), value.clone());
                }
            }
        }

        for key in ["env", "globals", "ignorePatterns", "settings"] {
            if let Some(value) = project.get(key) {
                merged.insert(key.to_string(), value.clone());
            }
        }

        if let Some(Value::Array(overrides)) = project.get("overrides") {
            let mut kept = Vec::new();
            for (index, entry) in overrides.iter().enumerate() {
                let Some(entry) = entry.as_object() else {
                    continue;
                };
                let mut filtered = Map::new();
                if let Some(files) = entry.get("files") {
                    filtered.insert("files".to_string(), files.clone());
                }
                if let Some(Value::Object(entry_rules)) = entry.get("rules") {
                    let mut kept_rules = Map::new();
                    for spec in RULE_REGISTRY {
                        if let Some(value) = entry_rules.get(spec.name) {
                            kept_rules.insert(spec.name.to_string(), value.clone());
                        }
                    }
                    if !kept_rules.is_empty() {
                        filtered.insert("rules".to_string(), Value::Object(kept_rules));
                    }
                }
                if filtered.contains_key("rules") || filtered.contains_key("files") {
                    if !filtered.contains_key("files") {
                        return Err(EngineError::Config(format!(
                            "{}: overrides[{index}] has registry rules but no `files` glob",
                            path.display()
                        )));
                    }
                    kept.push(Value::Object(filtered));
                }
            }
            if !kept.is_empty() {
                merged.insert("overrides".to_string(), Value::Array(kept));
            }
        }
    }

    merged.insert("rules".to_string(), Value::Object(rules));
    serde_json::to_string(&Value::Object(merged))
        .map_err(|error| EngineError::Config(format!("Unable to serialize lint config: {error}")))
}

// ---------------------------------------------------------------------------
// Format configuration
// ---------------------------------------------------------------------------

/// Options resolved for one file: engine options plus the one tsrx_format-side
/// option that is not part of `oxc_adapter::FormatOptions`.
#[derive(Debug, Clone, Default)]
pub struct FileFormatOptions {
    pub engine: FormatOptions,
    pub insert_final_newline: Option<bool>,
}

impl FileFormatOptions {
    /// Merges `other` over `self`, exactly like upstream `FileFormatOptions::merge`:
    /// only fields the other side actually sets win.
    pub fn merge(&mut self, other: &Self) {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(if other.engine.$field.is_some() {
                    self.engine.$field.clone_from(&other.engine.$field);
                })+
            };
        }
        merge!(
            use_tabs,
            tab_width,
            end_of_line,
            print_width,
            single_quote,
            jsx_single_quote,
            quote_props,
            trailing_comma,
            semi,
            arrow_parens,
            bracket_spacing,
            bracket_same_line,
            object_wrap,
            single_attribute_per_line,
            embedded_language_formatting,
            html_whitespace_sensitivity,
            jsdoc,
            sort_imports,
        );
        if other.insert_final_newline.is_some() {
            self.insert_final_newline = other.insert_final_newline;
        }
    }
}

struct FormatOverride {
    files: GlobSet,
    exclude_files: GlobSet,
    options: FileFormatOptions,
}

/// One resolved formatter configuration shared by a batch.
pub struct FormatConfig {
    config_root: PathBuf,
    base: FileFormatOptions,
    overrides: Vec<FormatOverride>,
    ignore: Option<Gitignore>,
}

impl FormatConfig {
    /// Resolves format options for `cwd`: blessed defaults, then a discovered or
    /// explicit `.oxfmtrc`, then `caller_options`.
    ///
    /// `caller_options` is a camelCase `.oxfmtrc`-shaped object; explicit CLI
    /// options ride on it and therefore win over file configuration.
    pub fn new(
        cwd: &Path,
        explicit_path: Option<&Path>,
        caller_options: Option<&Value>,
    ) -> Result<Self, EngineError> {
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        reject_editorconfig(&cwd, explicit_path)?;

        let mut base = blessed_defaults()?;
        let mut overrides = Vec::new();
        let mut ignore_patterns: Vec<String> = Vec::new();
        let mut config_root = cwd.clone();

        let config_path = explicit_path
            .map(|path| absolutize(&cwd, path))
            .or_else(|| discover_upward(&cwd, OXFMT_CONFIG_NAMES));
        if let Some(path) = config_path {
            if is_js_config(&path) {
                return Err(EngineError::Config(format!(
                    "JavaScript/TypeScript format config modules are not supported: {}",
                    path.display()
                )));
            }
            let source = fs::read_to_string(&path).map_err(|error| {
                EngineError::Config(format!("Unable to read {}: {error}", path.display()))
            })?;
            let value = parse_jsonc(&source).map_err(|detail| {
                EngineError::Config(format!("Invalid {}: {detail}", path.display()))
            })?;
            let raw: RawOxfmtrc = serde_json::from_value(value).map_err(|error| {
                EngineError::Config(format!("Invalid {}: {error}", path.display()))
            })?;
            let file_options = resolve_options(raw.options, &path)?;
            base.merge(&file_options);
            for (index, raw_override) in raw.overrides.into_iter().enumerate() {
                overrides.push(build_override(raw_override, &path, index)?);
            }
            ignore_patterns = raw.ignore_patterns;
            config_root = path.parent().unwrap_or(&cwd).to_path_buf();
        }

        if let Some(caller) = caller_options {
            let raw: RawFormatOptions =
                serde_json::from_value(caller.clone()).map_err(|error| {
                    EngineError::Config(format!("Invalid caller format options: {error}"))
                })?;
            base.merge(&resolve_options(raw, Path::new("<options>"))?);
        }

        let ignore = build_ignore(&config_root, &ignore_patterns)?;
        Ok(Self {
            config_root,
            base,
            overrides,
            ignore,
        })
    }

    /// Resolves the options one authored path formats with.
    pub fn options_for(&self, cwd: &Path, path: &Path) -> FileFormatOptions {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let relative = absolute
            .strip_prefix(&self.config_root)
            .unwrap_or(&absolute);
        let candidate = relative.to_string_lossy();
        let mut options = self.base.clone();
        for entry in &self.overrides {
            if entry.files.is_match(candidate.as_ref())
                && !entry.exclude_files.is_match(candidate.as_ref())
            {
                options.merge(&entry.options);
            }
        }
        options
    }

    /// Whether `path` matches the configuration's `ignorePatterns`.
    pub fn should_ignore(&self, cwd: &Path, path: &Path) -> bool {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let absolute = absolute.canonicalize().unwrap_or(absolute);
        if !absolute.starts_with(&self.config_root) {
            return false;
        }
        self.ignore.as_ref().is_some_and(|ignore| {
            ignore
                .matched_path_or_any_parents(absolute, false)
                .is_ignore()
        })
    }
}

fn blessed_defaults() -> Result<FileFormatOptions, EngineError> {
    let mut options = FileFormatOptions::default();
    for &(key, ref value) in BLESSED_FORMAT {
        set_option(&mut options, key, value, "blessed defaults")?;
    }
    Ok(options)
}

/// The `.oxfmtrc` file shape: options at the root plus `overrides` and
/// `ignorePatterns`, mirroring upstream `RawOxfmtrc`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawOxfmtrc {
    #[serde(flatten)]
    options: RawFormatOptions,
    overrides: Vec<RawFormatOverride>,
    ignore_patterns: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawFormatOverride {
    files: Vec<String>,
    exclude_files: Vec<String>,
    #[serde(flatten)]
    options: RawFormatOptions,
}

/// One option block, kept as typed fields so unknown keys are refused like
/// upstream `FormatError::UnknownOption`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawFormatOptions {
    #[serde(rename = "$schema")]
    schema: Option<Value>,
    use_tabs: Option<bool>,
    tab_width: Option<u8>,
    end_of_line: Option<String>,
    print_width: Option<u16>,
    single_quote: Option<bool>,
    jsx_single_quote: Option<bool>,
    quote_props: Option<String>,
    trailing_comma: Option<String>,
    semi: Option<bool>,
    arrow_parens: Option<String>,
    bracket_spacing: Option<bool>,
    bracket_same_line: Option<bool>,
    object_wrap: Option<String>,
    single_attribute_per_line: Option<bool>,
    embedded_language_formatting: Option<Value>,
    html_whitespace_sensitivity: Option<String>,
    insert_final_newline: Option<bool>,
    #[serde(alias = "experimentalSortImports")]
    sort_imports: Option<Value>,
    sort_tailwindcss: Option<Value>,
    jsdoc: Option<Value>,
    experimental_operator_position: Option<Value>,
    experimental_ternaries: Option<Value>,
    prose_wrap: Option<Value>,
    sort_package_json: Option<Value>,
    svelte: Option<Value>,
    vue_indent_script_and_style: Option<Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

fn resolve_options(raw: RawFormatOptions, path: &Path) -> Result<FileFormatOptions, EngineError> {
    let at = path.display();
    if let Some((option, _)) = raw.unknown.into_iter().next() {
        return Err(EngineError::Config(format!(
            "{at}: unknown option `{option}`"
        )));
    }
    if raw.embedded_language_formatting.is_some() {
        return Err(EngineError::Config(format!(
            "{at}: `embeddedLanguageFormatting` is unavailable in this formatter"
        )));
    }
    if raw.experimental_operator_position.is_some() || raw.experimental_ternaries.is_some() {
        return Err(EngineError::Config(format!(
            "{at}: experimental operator/ternary options are not supported"
        )));
    }
    if raw
        .sort_tailwindcss
        .is_some_and(|value| !value.is_null() && value != Value::Bool(false))
    {
        return Err(EngineError::Config(format!(
            "{at}: `sortTailwindcss` is not supported"
        )));
    }

    let mut options = FileFormatOptions {
        engine: FormatOptions {
            use_tabs: raw.use_tabs,
            tab_width: raw.tab_width,
            end_of_line: raw.end_of_line,
            print_width: raw.print_width,
            single_quote: raw.single_quote,
            jsx_single_quote: raw.jsx_single_quote,
            quote_props: raw.quote_props,
            trailing_comma: raw.trailing_comma,
            semi: raw.semi,
            arrow_parens: raw.arrow_parens,
            bracket_spacing: raw.bracket_spacing,
            bracket_same_line: raw.bracket_same_line,
            object_wrap: raw.object_wrap,
            single_attribute_per_line: raw.single_attribute_per_line,
            embedded_language_formatting: None,
            html_whitespace_sensitivity: raw.html_whitespace_sensitivity,
            jsdoc: None,
            sort_imports: None,
        },
        insert_final_newline: raw.insert_final_newline,
    };
    let _language_irrelevant = (
        raw.schema,
        raw.prose_wrap,
        raw.sort_package_json,
        raw.svelte,
        raw.vue_indent_script_and_style,
    );
    if let Some(jsdoc) = raw.jsdoc {
        options
            .engine
            .set_jsdoc(&jsdoc)
            .map_err(|error| EngineError::Config(format!("{at}: {error}")))?;
    }
    if let Some(sort_imports) = raw.sort_imports {
        options
            .engine
            .set_sort_imports(&sort_imports)
            .map_err(|error| EngineError::Config(format!("{at}: {error}")))?;
    }
    Ok(options)
}

/// Sets one named camelCase option, used for the in-code blessed defaults.
fn set_option(
    options: &mut FileFormatOptions,
    key: &str,
    value: &Value,
    scope: &str,
) -> Result<(), EngineError> {
    let raw = serde_json::from_value::<RawFormatOptions>(serde_json::json!({ key: value }))
        .map_err(|error| EngineError::Config(format!("{scope}: invalid `{key}`: {error}")))?;
    let resolved = resolve_options(raw, Path::new(scope))?;
    options.merge(&resolved);
    Ok(())
}

fn build_override(
    raw: RawFormatOverride,
    path: &Path,
    index: usize,
) -> Result<FormatOverride, EngineError> {
    if raw.files.is_empty() {
        return Err(EngineError::Config(format!(
            "{}: overrides[{index}] must list `files`",
            path.display()
        )));
    }
    Ok(FormatOverride {
        files: build_globs(&raw.files, path)?,
        exclude_files: build_globs(&raw.exclude_files, path)?,
        options: resolve_options(raw.options, path)?,
    })
}

fn build_globs(patterns: &[String], path: &Path) -> Result<GlobSet, EngineError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            EngineError::Config(format!(
                "{}: invalid glob `{pattern}`: {error}",
                path.display()
            ))
        })?);
    }
    builder.build().map_err(|error| {
        EngineError::Config(format!(
            "{}: cannot build glob set: {error}",
            path.display()
        ))
    })
}

fn build_ignore(root: &Path, patterns: &[String]) -> Result<Option<Gitignore>, EngineError> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        builder.add_line(None, pattern).map_err(|error| {
            EngineError::Config(format!("invalid ignore pattern `{pattern}`: {error}"))
        })?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|error| EngineError::Config(format!("cannot build ignore set: {error}")))
}

// ---------------------------------------------------------------------------
// Shared discovery helpers
// ---------------------------------------------------------------------------

fn absolutize(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Walks `start` and each ancestor looking for a known config file name.
/// Mirrors upstream's refusal when several config files sit in one directory.
fn discover_upward(start: &Path, names: &[&str]) -> Option<PathBuf> {
    let mut directory = start.to_path_buf();
    loop {
        let mut found: Vec<PathBuf> = names
            .iter()
            .map(|name| directory.join(name))
            .filter(|path| path.is_file())
            .collect();
        if let Some(path) = found.pop() {
            return Some(path.canonicalize().unwrap_or(path));
        }
        if !directory.pop() {
            return None;
        }
    }
}

fn is_js_config(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| JS_CONFIG_EXTENSIONS.contains(&extension))
}

/// `.editorconfig` silently changes what a formatter emits; upstream refuses to
/// format when one is in scope, and this resolver keeps that refusal.
fn reject_editorconfig(cwd: &Path, explicit: Option<&Path>) -> Result<(), EngineError> {
    if explicit.is_some_and(|path| path.file_name().is_some_and(|name| name == ".editorconfig")) {
        return Err(EngineError::Config(
            ".editorconfig cannot be used as the format configuration".to_string(),
        ));
    }
    let mut directory = cwd.to_path_buf();
    loop {
        let path = directory.join(".editorconfig");
        if path.is_file() {
            return Err(EngineError::Config(format!(
                ".editorconfig is not supported by this formatter: {}",
                path.display()
            )));
        }
        if !directory.pop() {
            return Ok(());
        }
    }
}

/// JSONC → JSON via `serde_json`'s own parser on a comment-stripped buffer.
/// Upstream strips with `json-strip-comments`; a line/block comment stripper
/// plus trailing commas covers the same surface for these config files.
fn parse_jsonc(source: &str) -> Result<Value, String> {
    let stripped = strip_jsonc(source);
    serde_json::from_str(&stripped).map_err(|error| error.to_string())
}

fn strip_jsonc(source: &str) -> String {
    let mut buffer = source.to_string();
    if json_strip_comments::strip(&mut buffer).is_err() {
        return source.to_string();
    }
    buffer
}

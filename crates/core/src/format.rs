//! The format stage: canonical Oxfmt for ordinary source, and the upstream
//! TSRX project → format → lift pipeline for `.tsrx`, both driven by the
//! programmatic `FormatOptions` the upstream session type cannot take.
//!
//! `format_once`, `settle`, and `apply_final_newline` mirror
//! `tsrx_format`'s internals against the same pinned public building blocks
//! (`scan_for_parser`, `project_for_format`, `oxc_adapter::format`,
//! `lift_formatted`, `count_expanded_objects`). The smallest upstream change
//! that would retire this mirror is a `FormatSession` constructor accepting
//! an in-memory config or an `oxc_adapter::FormatOptions` value; until that
//! exists this composition keeps projection, lift-time validation, and the
//! engine itself upstream-owned.

use std::path::Path;

use oxc_adapter::{DynamicTagContract, FormatRequest, SourceKind};
use tsrx_syntax::{lift_formatted, project_for_format, scan_for_parser};

use crate::{
    config::FileFormatOptions,
    diagnostic::{Diagnostic, Stage},
};

/// The most formatter passes [`settle`] takes before returning what it has.
/// Mirrors upstream `MAX_FORMAT_PASSES`.
const MAX_FORMAT_PASSES: u32 = 3;

/// What one pass leaves behind for [`settle`]: the legal TSX the engine
/// printed (projected for TSRX) and how many pre-expanded objects its input
/// held.
struct DriftProbe {
    formatted: String,
    source_kind: SourceKind,
    tsrx: bool,
    expanded_before: u32,
}

impl DriftProbe {
    /// One parse of the formatted text; like upstream, unparsable output is
    /// treated as drifting so the next pass surfaces the real error.
    fn may_drift(&self) -> bool {
        match oxc_adapter::count_expanded_objects(&self.formatted, self.source_kind, self.tsrx) {
            Ok(after) => after > self.expanded_before,
            Err(_) => true,
        }
    }
}

/// Formats caller-owned text with resolved per-file options.
///
/// A lone `\r` is normalized to `\n` first, matching upstream
/// `format_text_with_options`: Oxfmt reads a bare CR in JSX text as a space.
pub fn format_source(
    path: &Path,
    source: &str,
    options: &FileFormatOptions,
) -> Result<String, Vec<Diagnostic>> {
    let display = path.display().to_string();
    let run = |input: &str| -> Result<String, Vec<Diagnostic>> {
        if contains_bare_carriage_return(input) {
            let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
            settle(&normalized, |text| format_once(path, text, options))
        } else {
            settle(input, |text| format_once(path, text, options))
        }
    };
    run(source).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|mut diagnostic| {
                if diagnostic.path.is_empty() {
                    diagnostic.path.clone_from(&display);
                }
                diagnostic
            })
            .collect()
    })
}

/// Formats again while a pass could have changed what the next one reads.
/// Mirrors upstream `settle_format`, including the CR-terminated early return.
fn settle(
    source: &str,
    mut format: impl FnMut(&str) -> Result<(String, DriftProbe), Vec<Diagnostic>>,
) -> Result<String, Vec<Diagnostic>> {
    let (mut code, mut probe) = format(source)?;
    if contains_bare_carriage_return(&code) {
        return Ok(code);
    }
    let mut pass_count = 1u32;
    while code != source && pass_count < MAX_FORMAT_PASSES {
        if !probe.may_drift() {
            break;
        }
        let (next, next_probe) = format(&code)?;
        probe = next_probe;
        pass_count += 1;
        if next == code {
            break;
        }
        code = next;
    }
    Ok(code)
}

fn format_once(
    path: &Path,
    source: &str,
    options: &FileFormatOptions,
) -> Result<(String, DriftProbe), Vec<Diagnostic>> {
    let display = path.display().to_string();
    let diagnostic = |code: &'static str, message: String| {
        vec![Diagnostic::new(&display, Stage::Format, code, message)]
    };
    let at = |diagnostic: Diagnostic, offset: u32| diagnostic.at(source, offset);

    if path
        .extension()
        .is_some_and(|extension| extension == "tsrx")
    {
        let overlay = scan_for_parser(source)
            .map_err(|error| vec![projection_diagnostic(&display, source, &error)])?;
        let projection = project_for_format(source, &overlay)
            .map_err(|error| vec![projection_diagnostic(&display, source, &error)])?;
        let engine = oxc_adapter::format(&FormatRequest {
            parse_source: projection.source(),
            source_kind: SourceKind::TypeScriptReact,
            dynamic_tags: projection
                .dynamic_contract()
                .map(|(prefix, count, offsets)| DynamicTagContract {
                    prefix,
                    count,
                    original_offsets: offsets,
                }),
            options: Some(&options.engine),
            tsrx: true,
        })
        .map_err(|error| diagnostic("OXC_FORMAT", error.to_string()))?;
        let code = lift_formatted(&engine.code, source, &projection).map_err(|error| {
            vec![at(
                Diagnostic::new(&display, Stage::Format, "TSRX_LIFT", error.to_string()),
                projection_error_offset(&error).unwrap_or(0),
            )]
        })?;
        let code = apply_final_newline(code, options);
        return Ok((
            code,
            DriftProbe {
                formatted: engine.code,
                source_kind: SourceKind::TypeScriptReact,
                tsrx: true,
                expanded_before: engine.expanded_objects,
            },
        ));
    }

    let source_kind = SourceKind::from_path(path)
        .map_err(|error| diagnostic("UNSUPPORTED_EXTENSION", error.to_string()))?;
    let engine = oxc_adapter::format(&FormatRequest {
        parse_source: source,
        source_kind,
        dynamic_tags: None,
        options: Some(&options.engine),
        tsrx: false,
    })
    .map_err(|error| diagnostic("OXC_FORMAT", error.to_string()))?;
    let code = apply_final_newline(engine.code, options);
    Ok((
        code.clone(),
        DriftProbe {
            formatted: code,
            source_kind,
            tsrx: false,
            expanded_before: engine.expanded_objects,
        },
    ))
}

fn apply_final_newline(mut code: String, options: &FileFormatOptions) -> String {
    if options.insert_final_newline == Some(false) {
        while code.ends_with('\n') || code.ends_with('\r') {
            code.pop();
        }
    }
    code
}

/// Whether `text` contains a `\r` that is not part of a `\r\n` pair.
fn contains_bare_carriage_return(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
}

fn projection_diagnostic(
    path: &str,
    source: &str,
    error: &tsrx_syntax::ProjectionError,
) -> Diagnostic {
    let diagnostic = Diagnostic::new(path, Stage::Parse, "TSRX_PROJECTION", error.to_string());
    match projection_error_offset(error) {
        Some(offset) => diagnostic.at(source, offset),
        None => diagnostic,
    }
}

fn projection_error_offset(error: &tsrx_syntax::ProjectionError) -> Option<u32> {
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

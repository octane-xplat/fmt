//! Collecting [`Container`]s: an ordered list of statement spans per
//! statement-list node, plus the implicit render boundary of TSRX
//! `JSXCodeBlock`s. Ordinary source walks the typed OXC AST; TSRX walks the
//! reconstructed `FlatTape`, which carries the same `ESTree` node names and
//! authored coordinates.

use std::path::Path;

use tsrx_parser_engine::{TsrxParseRequest, TsrxUtf16ParseRequest, parse_tsrx, parse_tsrx_utf16};
use tsrx_tape_schema::{CoordinateDomain, FlatTape, ParseCompleteness, RecordIndex, ValueKind};

use crate::{
    diagnostic::{Diagnostic, Stage},
    spacing::{Container, StmtSpan},
    utf16::Utf16Index,
};

/// Collects spacing containers for `path`/`source`.
///
/// Ordinary files parse through the pinned `oxc_parser`; `.tsrx` files go
/// through the canonical TSRX reconstruction (UTF-8 tape for ASCII sources,
/// the UTF-16 route otherwise, with spans mapped back to authored bytes).
///
/// # Errors
///
/// Returns diagnostics when the source does not parse or reconstruct.
pub fn collect_containers(path: &Path, source: &str) -> Result<Vec<Container>, Vec<Diagnostic>> {
    if path
        .extension()
        .is_some_and(|extension| extension == "tsrx")
    {
        collect_tsrx(path, source)
    } else {
        collect_ordinary(path, source)
    }
}

// ---------------------------------------------------------------------------
// Ordinary JS/TS: typed AST
// ---------------------------------------------------------------------------

fn collect_ordinary(path: &Path, source: &str) -> Result<Vec<Container>, Vec<Diagnostic>> {
    use oxc_ast::ast::{
        BlockStatement, Directive, Program, Statement, StaticBlock, SwitchCase, TSModuleBlock,
    };
    use oxc_ast_visit::{Visit, walk};
    use oxc_span::GetSpan;

    struct Collector {
        containers: Vec<Container>,
    }

    impl Collector {
        fn push<'a>(&mut self, statements: &[Statement<'a>], directives: &[Directive<'a>]) {
            self.containers.push(Container {
                statements: directives
                    .iter()
                    .map(Directive::span)
                    .chain(statements.iter().map(Statement::span))
                    .map(|span| StmtSpan {
                        start: span.start,
                        end: span.end,
                    })
                    .collect(),
                render_start: None,
            });
        }
    }

    impl<'a> Visit<'a> for Collector {
        fn visit_program(&mut self, program: &Program<'a>) {
            self.push(&program.body, &program.directives);
            walk::walk_program(self, program);
        }
        fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
            self.push(&block.body, &[]);
            walk::walk_block_statement(self, block);
        }
        fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
            self.push(&block.body, &[]);
            walk::walk_static_block(self, block);
        }
        fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
            self.push(&case.consequent, &[]);
            walk::walk_switch_case(self, case);
        }
        fn visit_ts_module_block(&mut self, block: &TSModuleBlock<'a>) {
            self.push(&block.body, &block.directives);
            walk::walk_ts_module_block(self, block);
        }
    }

    let display = path.display().to_string();
    let source_type = oxc_span::SourceType::from_path(path).map_err(|_| {
        vec![Diagnostic::new(
            &display,
            Stage::Parse,
            "UNSUPPORTED_EXTENSION",
            format!("Unsupported source extension: {:?}", path.extension()),
        )]
    })?;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(parsed
            .diagnostics
            .iter()
            .map(|error| {
                let offset = error
                    .labels
                    .iter()
                    .next()
                    .map(oxc_span::LabeledSpan::offset);
                let diagnostic =
                    Diagnostic::new(&display, Stage::Parse, "OXC_PARSE", error.to_string());
                match offset {
                    Some(offset) => diagnostic.at(source, offset),
                    None => diagnostic,
                }
            })
            .collect());
    }

    let mut collector = Collector {
        containers: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    Ok(collector.containers)
}

// ---------------------------------------------------------------------------
// TSRX: FlatTape walk
// ---------------------------------------------------------------------------

/// The statement-list fields the JS rule walked, by `ESTree` node type.
fn statement_list_fields(node_type: &str) -> &'static [&'static str] {
    match node_type {
        "Program" | "BlockStatement" | "StaticBlock" | "TSModuleBlock" | "JSXCodeBlock" => {
            &["body"]
        }
        "SwitchCase" => &["consequent"],
        _ => &[],
    }
}

/// Tightens a tape span to its non-whitespace bounds. Tape node spans cover
/// an authored line region, indent and trailing trivia included; the spacing
/// rules were written against tight statement spans, so the boundary between
/// two statements must land on real text for the gap rules to see the
/// separating line break.
fn trim_span(source: &str, start: u32, end: u32) -> StmtSpan {
    let bytes = source.as_bytes();
    let mut start = start as usize;
    let mut end = (end as usize).min(bytes.len());
    while start < end
        && source[start..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    {
        start += 1;
    }
    while end > start
        && source[..end]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        end -= 1;
    }
    StmtSpan {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    }
}

/// Turns an incomplete parse result into structured diagnostics.
fn failure_diagnostics(
    display: &str,
    source: &str,
    result: &tsrx_parser_engine::TsrxParseResult,
    to_byte: &dyn Fn(u32) -> Option<u32>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for record in result.errors.records() {
        let message = result
            .errors
            .string(record.message)
            .unwrap_or("TSRX parse failed")
            .to_string();
        let mut diagnostic = Diagnostic::new(display, Stage::Parse, "TSRX_PARSE", message);
        if let Some(labels) = result.errors.labels(record.labels)
            && let Some(label) = labels.iter().find(|label| label.primary).or(labels.first())
            && let Some(offset) = to_byte(label.span.start)
        {
            diagnostic = diagnostic.at(source, offset);
        }
        diagnostics.push(diagnostic);
    }
    if diagnostics.is_empty() {
        diagnostics.push(Diagnostic::new(
            display,
            Stage::Parse,
            "TSRX_PARSE",
            "TSRX source did not parse completely",
        ));
    }
    diagnostics
}

fn collect_tsrx(path: &Path, source: &str) -> Result<Vec<Container>, Vec<Diagnostic>> {
    let display = path.display().to_string();

    // The UTF-8 route rejects non-ASCII; the UTF-16 route returns spans in
    // UTF-16 units, mapped back to bytes by `index`.
    let parse_error = |error: tsrx_parser_engine::TsrxParseError| {
        vec![Diagnostic::new(
            &display,
            Stage::Parse,
            "TSRX_PARSE",
            error.to_string(),
        )]
    };
    let (result, index): (tsrx_parser_engine::TsrxParseResult, Option<Utf16Index>) =
        if source.is_ascii() {
            (
                parse_tsrx(&TsrxParseRequest { source }).map_err(parse_error)?,
                None,
            )
        } else {
            let (index, units) = Utf16Index::new(source);
            (
                parse_tsrx_utf16(&TsrxUtf16ParseRequest { source: &units }).map_err(parse_error)?,
                Some(index),
            )
        };

    let to_byte = |offset: u32| -> Option<u32> {
        match &index {
            Some(index) => index.byte_at(offset),
            None => Some(offset),
        }
    };

    if result.status != ParseCompleteness::Complete || result.program.is_none() {
        return Err(failure_diagnostics(&display, source, &result, &to_byte));
    }

    debug_assert!(
        matches!(
            result.coordinate_domain,
            CoordinateDomain::AuthoredUtf8Bytes | CoordinateDomain::OriginalUtf16Units
        ),
        "unexpected tape coordinate domain"
    );

    let mut containers = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let tape = result.program();
    let mut stack = vec![tape.root()];
    while let Some(value) = stack.pop() {
        let Some(object) = value.as_object() else {
            continue;
        };
        if !visited.insert(object) {
            continue;
        }
        let Some(node_type) = node_type(tape, object) else {
            // Every AST node object carries a `type`; field-less wrappers are
            // still walked below via their children.
            push_children(tape, object, &mut stack);
            continue;
        };
        for field in statement_list_fields(&node_type) {
            let mut statements = Vec::new();
            if let Some(list) = field_list(tape, object, field) {
                for item in tape.values(list) {
                    if item.kind() != ValueKind::Object {
                        continue;
                    }
                    if let (Some(start), Some(end)) =
                        (object_start(tape, item), object_end(tape, item))
                        && let (Some(start), Some(end)) = (to_byte(start), to_byte(end))
                    {
                        statements.push(trim_span(source, start, end));
                    }
                }
            }
            let render_start = if node_type == "JSXCodeBlock" {
                field_value(tape, object, "render")
                    .and_then(|value| object_start(tape, value))
                    .and_then(&to_byte)
            } else {
                None
            };
            containers.push(Container {
                statements,
                render_start,
            });
        }
        push_children(tape, object, &mut stack);
    }
    Ok(containers)
}

/// Reads a node's `type` field. Tape scalars store JSON spellings, so the
/// value arrives quoted.
fn node_type(tape: &FlatTape, object: RecordIndex) -> Option<String> {
    let value = field_value(tape, object, "type")?;
    let raw = tape.scalar(value)?;
    Some(raw.trim_matches('"').to_string())
}

fn field_value(
    tape: &FlatTape,
    object: RecordIndex,
    name: &str,
) -> Option<tsrx_tape_schema::ValueRef> {
    tape.field_index(object, name)
        .and_then(|field| tape.field_value(field))
}

fn field_list(tape: &FlatTape, object: RecordIndex, name: &str) -> Option<RecordIndex> {
    field_value(tape, object, name).and_then(tsrx_tape_schema::ValueRef::as_list)
}

fn object_start(tape: &FlatTape, value: tsrx_tape_schema::ValueRef) -> Option<u32> {
    field_value(tape, value.as_object()?, "start").and_then(|value| tape.scalar_u32(value))
}

fn object_end(tape: &FlatTape, value: tsrx_tape_schema::ValueRef) -> Option<u32> {
    field_value(tape, value.as_object()?, "end").and_then(|value| tape.scalar_u32(value))
}

/// Queues every child node object, mirroring the JS `childNodes` walk:
/// object fields and lists of objects, skipping `parent`, `loc`, `range`.
fn push_children(
    tape: &FlatTape,
    object: RecordIndex,
    stack: &mut Vec<tsrx_tape_schema::ValueRef>,
) {
    let _ = tape;
    for field in tape.fields(object) {
        let key = tape.key(field);
        if matches!(key, "parent" | "loc" | "range") {
            continue;
        }
        match field.value.kind() {
            ValueKind::Object => stack.push(field.value),
            ValueKind::List => {
                if let Some(list) = field.value.as_list() {
                    for item in tape.values(list) {
                        if item.kind() == ValueKind::Object {
                            stack.push(item);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

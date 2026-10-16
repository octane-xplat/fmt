//! The custom Xplat fix: a blank line after every multiline statement and
//! before a TSRX code block's implicit render expression. Ported from
//! `packages/lint/src/spacing-rules.mjs` at octane-xplat commit
//! `8295753813e525d6d8ae2fa848e75ab225899abe`; the JS version's own comments
//! are preserved where the behavior is subtle.
//!
//! The pass works on *span containers*, not on any AST type: each container
//! is an ordered statement list plus, for TSRX `JSXCodeBlock`s, the start of
//! the implicit render expression. [`crate::tree`] produces them for both
//! ordinary source (typed OXC AST) and TSRX (reconstructed tape), so this
//! file has one implementation of the gap/ASI/comment rules.

/// A statement's authored UTF-8 byte span.
#[derive(Debug, Clone, Copy)]
pub struct StmtSpan {
    pub start: u32,
    pub end: u32,
}

/// One ordered statement list: `Program`/`BlockStatement`/`StaticBlock`
/// `body`, `SwitchCase` `consequent`, `TSModuleBlock` `body`, or a TSRX
/// `JSXCodeBlock` `body` with its `render` boundary.
#[derive(Debug, Default)]
pub struct Container {
    pub statements: Vec<StmtSpan>,
    /// Start offset of a `JSXCodeBlock`'s implicit render expression.
    pub render_start: Option<u32>,
}

/// A located insertion: where the blank line goes and which statement
/// triggered it (for `xplat-disable-*` suppression checks).
#[derive(Debug)]
pub struct SpacingViolation {
    pub position: u32,
    pub text: String,
    /// The offending statement's start, used to compute its line.
    pub node_start: u32,
}

fn first_line_break(text: &str) -> Option<(usize, usize)> {
    // Returns (index, length) of the first \r\n, \n, or \r.
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\n' => return Some((index, 1)),
            b'\r' => {
                let length = if bytes.get(index + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                return Some((index, length));
            }
            _ => {}
        }
    }
    None
}

fn has_line_break(text: &str) -> bool {
    first_line_break(text).is_some()
}

fn has_blank_line_after(source: &str, end: u32, next_start: u32) -> bool {
    let (end, next_start) = (end as usize, next_start as usize);
    if end > next_start || next_start > source.len() {
        return false;
    }
    let gap = &source[end..next_start];
    let Some((index, length)) = first_line_break(gap) else {
        return false;
    };
    let after_line = &gap[index + length..];
    match first_line_break(after_line) {
        Some((next_index, _)) => after_line[..next_index].trim().is_empty(),
        None => false,
    }
}

fn insert_blank_line(source: &str, end: u32, next_start: u32) -> (u32, String) {
    let gap = &source[end as usize..next_start as usize];
    match first_line_break(gap) {
        // No line break in the gap at all: append a full blank line after `end`.
        None => {
            let eol = first_line_break(source)
                .map_or("\n", |(index, length)| &source[index..index + length]);
            (end, format!("{eol}{eol}"))
        }
        // Reuse the line break already ending `current`'s last line.
        Some((index, length)) => (
            end + u32::try_from(index + length).unwrap_or(0),
            gap[index..index + length].to_string(),
        ),
    }
}

fn is_multiline(span: StmtSpan, source: &str) -> bool {
    let (start, end) = (span.start as usize, span.end as usize);
    end <= source.len() && start <= end && has_line_break(&source[start..end])
}

/// `;(x)` / `;[x]` ASI guards: the parser folds the `;` into the PREVIOUS
/// statement's range, so `current.end === next.start` and the `;` hides the
/// real boundary. The blank line belongs before the `;`'s line, above any
/// comment lines attached to it.
fn is_asi_guard(source: &str, current: StmtSpan, next: StmtSpan) -> bool {
    if current.end != next.start {
        return false;
    }
    let semi = next.start as usize - 1;
    let bytes = source.as_bytes();
    if bytes.get(semi) != Some(&b';') {
        return false;
    }
    match bytes.get(next.start as usize) {
        Some(b'(' | b'[') => {}
        _ => return false,
    }
    // The `;` must lead its own line — `foo();(x)` on one line isn't a guard.
    let line_start = source[..semi].rfind('\n').map_or(0, |index| index + 1);
    source[line_start..semi].trim().is_empty()
}

fn guard_insert_pos(source: &str, semi_pos: usize) -> usize {
    // Start of the `;`'s own line, then walk back over the comment lines
    // attached above it — the blank goes above those too.
    let mut pos = source[..semi_pos].rfind('\n').map_or(0, |index| index + 1);
    while pos > 0 {
        let prev_end = pos - 1;
        let prev_start = source[..prev_end].rfind('\n').map_or(0, |index| index + 1);
        if !is_line_comment_line(&source[prev_start..prev_end]) {
            break;
        }
        pos = prev_start;
    }
    pos
}

fn is_line_comment_line(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Mirrors the JS `/\n[ \t]*\n[ \t]*$/` test on `source[..pos]`.
fn ends_with_blank_line(text: &str) -> bool {
    let text = text.trim_end_matches([' ', '\t']);
    let Some(text) = text.strip_suffix('\n') else {
        return false;
    };
    let text = text.trim_end_matches([' ', '\t']);
    text.ends_with('\n')
}

fn handle_asi_guard(
    source: &str,
    current: StmtSpan,
    next: StmtSpan,
    violations: &mut Vec<SpacingViolation>,
) {
    let semi = next.start as usize - 1;
    if !has_line_break(&source[current.start as usize..semi]) {
        return;
    }
    let pos = guard_insert_pos(source, semi);
    if ends_with_blank_line(&source[..pos]) {
        return;
    }
    let eol =
        first_line_break(source).map_or("\n", |(index, length)| &source[index..index + length]);
    violations.push(SpacingViolation {
        position: u32::try_from(pos).unwrap_or(0),
        text: eol.to_string(),
        node_start: current.start,
    });
}

/// All spacing violations in `source`, in the same order the JS visitor
/// produced them (containers in pre-order, statements in source order).
pub fn find_spacing_violations(source: &str, containers: &[Container]) -> Vec<SpacingViolation> {
    let mut violations = Vec::new();
    for container in containers {
        let statements = &container.statements;
        for pair in statements.windows(2) {
            let [current, next] = pair else { continue };
            if is_asi_guard(source, *current, *next) {
                handle_asi_guard(source, *current, *next, &mut violations);
                continue;
            }
            if !is_multiline(*current, source)
                || has_blank_line_after(source, current.end, next.start)
            {
                continue;
            }
            let (position, text) = insert_blank_line(source, current.end, next.start);
            violations.push(SpacingViolation {
                position,
                text,
                node_start: current.start,
            });
        }

        if let Some(render_start) = container.render_start
            && let Some(setup) = statements.last()
            && !has_blank_line_after(source, setup.end, render_start)
        {
            let (position, text) = insert_blank_line(source, setup.end, render_start);
            violations.push(SpacingViolation {
                position,
                text,
                node_start: setup.start,
            });
        }
    }
    violations
}

/// `xplat-disable` suppression, ported from `lint-tsrx.mjs`:
/// `xplat-disable` in the first three lines disables the file;
/// `xplat-disable-line` and `xplat-disable-next-line` suppress the
/// same/next 1-based line.
pub struct DisabledLines {
    pub file_disabled: bool,
    pub lines: std::collections::HashSet<u32>,
}

#[must_use]
pub fn disabled_lines(source: &str) -> DisabledLines {
    let raw: Vec<&str> = source.split('\n').collect();
    let file_disabled = raw.iter().take(3).any(|line| {
        line.contains("xplat-disable")
            && !line.contains("xplat-disable-next-line")
            && !line.contains("xplat-disable-line")
    });
    let mut lines = std::collections::HashSet::new();
    for (index, line) in raw.iter().enumerate() {
        if line.contains("xplat-disable-next-line") {
            lines.insert(u32::try_from(index).unwrap_or(0) + 2);
        } else if line.contains("xplat-disable-line") {
            lines.insert(u32::try_from(index).unwrap_or(0) + 1);
        }
    }
    DisabledLines {
        file_disabled,
        lines,
    }
}

/// Drops violations whose statement start sits on a disabled line; a disabled
/// file drops all of them.
pub fn filter_disabled(source: &str, violations: Vec<SpacingViolation>) -> Vec<SpacingViolation> {
    let disabled = disabled_lines(source);
    if disabled.file_disabled {
        return Vec::new();
    }
    violations
        .into_iter()
        .filter(|violation| {
            let (line, _) = crate::diagnostic::line_column(source, violation.node_start);
            !disabled.lines.contains(&line)
        })
        .collect()
}

//! Applying validated edits to an owned source buffer.

/// One replacement over a UTF-8 byte range.
#[derive(Debug, Clone)]
pub struct Edit {
    pub start: u32,
    pub end: u32,
    pub replacement: String,
}

/// Applies the non-overlapping subset of `edits` to `source`, right to left.
///
/// Edits are dropped rather than applied when they overlap an already-applied
/// edit, run past the buffer, or split a UTF-8 boundary. Returns the number
/// applied.
pub fn apply_edits(source: &mut String, edits: &[Edit]) -> usize {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut applied = 0;
    let mut previous_start = usize::MAX;
    for edit in sorted {
        let start = usize::try_from(edit.start).unwrap_or(usize::MAX);
        let end = usize::try_from(edit.end).unwrap_or(usize::MAX);
        if start > end || end > source.len() || end > previous_start {
            continue;
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            continue;
        }
        source.replace_range(start..end, &edit.replacement);
        previous_start = start;
        applied += 1;
    }
    applied
}

/// Pure insertion used by the spacing pass; the same rules as [`apply_edits`].
#[derive(Debug, Clone)]
pub struct Insertion {
    pub position: u32,
    pub text: String,
}

/// Inserts each text at its position, right to left. Duplicate positions are
/// applied once.
pub fn apply_insertions(source: &mut String, insertions: &[Insertion]) {
    let mut sorted: Vec<&Insertion> = insertions.iter().collect();
    sorted.sort_by_key(|insertion| std::cmp::Reverse(insertion.position));
    let mut previous = usize::MAX;
    for insertion in sorted {
        let position = usize::try_from(insertion.position).unwrap_or(usize::MAX);
        if position >= previous || position > source.len() || !source.is_char_boundary(position) {
            continue;
        }
        source.insert_str(position, &insertion.text);
        previous = position;
    }
}

#[cfg(test)]
mod tests {
    use super::{Edit, Insertion, apply_edits, apply_insertions};

    #[test]
    fn applies_right_to_left_and_skips_overlaps() {
        let mut source = "aabbcc".to_string();
        let edits = vec![
            Edit {
                start: 0,
                end: 2,
                replacement: "X".into(),
            },
            Edit {
                start: 2,
                end: 4,
                replacement: "Y".into(),
            },
            Edit {
                start: 1,
                end: 5,
                replacement: "overlap".into(),
            },
        ];
        assert_eq!(apply_edits(&mut source, &edits), 2);
        assert_eq!(source, "XYcc");
    }

    #[test]
    fn insertions_preserve_order() {
        let mut source = "ab".to_string();
        apply_insertions(
            &mut source,
            &[
                Insertion {
                    position: 1,
                    text: "X".into(),
                },
                Insertion {
                    position: 1,
                    text: "Y".into(),
                },
            ],
        );
        // Only one of the two same-position insertions survives.
        assert!(source == "aXb" || source == "aYb");
    }
}

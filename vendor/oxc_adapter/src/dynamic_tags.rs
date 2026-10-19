use std::{error::Error, fmt};

use oxc_ast::ast::{Expression, JSXAttributeValue, JSXOpeningElement};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};

use crate::DynamicTagContract;

/// `@tsrx/core`'s message for a dynamic tag expression that isn't one of the allowed forms.
pub const DYNAMIC_TAG_EXPRESSION_MESSAGE: &str = "A dynamic tag expression must be an identifier, a member access such as `props.as` or `registry[name]`, or a string literal. Compute anything else before the element: `const Tag = c ? Child : Fallback;`, then `<{Tag} />`.";

/// `@tsrx/core`'s diagnostic code for a dynamic tag expression that isn't one of the allowed forms.
pub const DYNAMIC_TAG_EXPRESSION_CODE: &str = "TSRX2014";

/// The part of one dynamic tag expression that isn't an allowed form, in projected-source bytes.
///
/// This is a recoverable report, not a parse failure: the parse keeps its Program, a strict
/// consumer throws it, and a collecting consumer records it and goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDynamicTag {
    /// The scaffold ordinal of the element whose opening tag holds the expression.
    pub index: usize,
    /// Projected-source start of the invalid part, including any parentheses around it.
    pub start: u32,
    /// Projected-source end of the invalid part, including any parentheses around it.
    pub end: u32,
}

/// Validates the dynamic-tag scaffolds of a toolchain (format or lint) projection and returns the
/// authored expressions `@tsrx/core` reports, in toolchain-projection bytes.
///
/// The toolchain lanes format and lint a file whose dynamic tag expressions the parser only
/// reports, as `@tsrx/core` formatters do, so only the scaffold contract can fail here. The lint
/// lane surfaces the reports as error diagnostics, as core's editor tooling does. The toolchain
/// projection writes the expression as `{expression}`, without the parser projection's generated
/// parentheses, so every parenthesis inside the container is authored.
#[cfg(feature = "toolchain")]
pub(crate) fn find_invalid_dynamic_tags(
    program: &oxc_ast::ast::Program<'_>,
    contract: Option<DynamicTagContract<'_>>,
) -> Result<Vec<InvalidDynamicTag>, DynamicTagError> {
    validate(program, contract, &[], false)
}

/// Why a TSRX dynamic-tag scaffold did not validate against the parsed OXC AST.
///
/// Every variant describes an inconsistent scaffold contract, which is a projector or adapter
/// defect rather than anything an author wrote. An authored expression that isn't an allowed tag
/// form is no error: `@tsrx/core` reports it without changing the parse, so it comes back as an
/// [`InvalidDynamicTag`] next to a valid scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicTagError {
    /// The synthetic callee spans were not handed over in ascending order.
    UnorderedSyntheticCallees,
    /// The contract claims more dynamic tags than this target can address.
    CountExceedsAddressableMemory,
    /// The contract has no prefix, no tags, or an offset list that does not match its count.
    EmptyContract,
    /// A prefixed element name did not parse as a scaffold ordinal.
    MalformedScaffold,
    /// A scaffold ordinal is out of range or was seen twice.
    InvalidScaffold { index: usize },
    /// Canonical OXC's parse did not preserve one of the scaffolds the projector emitted.
    LostScaffold { index: usize },
    /// The end sentinel does not belong to the scaffold that opened it.
    MismatchedEndScaffold { index: usize },
    /// A prefixed attribute name did not parse as a scaffold ordinal.
    MalformedAttribute { index: usize },
    /// The expression attribute does not belong to the scaffold that opened it.
    MismatchedAttribute { index: usize },
    /// The scaffold carries no dynamic-tag expression.
    MissingExpression { index: usize },
    /// The scaffold was never closed by its end sentinel.
    MissingEndScaffold { index: usize },
}

impl fmt::Display for DynamicTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnorderedSyntheticCallees => {
                formatter.write_str("unordered synthetic callee span contract")
            }
            Self::CountExceedsAddressableMemory => {
                formatter.write_str("TSRX dynamic-tag count exceeds addressable memory")
            }
            Self::EmptyContract => {
                formatter.write_str("invalid empty TSRX dynamic-tag scaffold contract")
            }
            Self::MalformedScaffold => formatter.write_str("malformed TSRX dynamic-tag scaffold"),
            Self::InvalidScaffold { index } => {
                write!(formatter, "invalid TSRX dynamic-tag scaffold {index}")
            }
            Self::LostScaffold { index } => {
                write!(formatter, "OXC parse lost TSRX dynamic-tag scaffold {index}")
            }
            Self::MismatchedEndScaffold { index } => {
                write!(formatter, "mismatched TSRX dynamic-tag end scaffold {index}")
            }
            Self::MalformedAttribute { index } => {
                write!(formatter, "malformed TSRX dynamic-tag attribute {index}")
            }
            Self::MismatchedAttribute { index } => {
                write!(formatter, "mismatched TSRX dynamic-tag attribute {index}")
            }
            Self::MissingExpression { index } => {
                write!(formatter, "missing TSRX dynamic-tag expression {index}")
            }
            Self::MissingEndScaffold { index } => {
                write!(formatter, "missing TSRX dynamic-tag end scaffold {index}")
            }
        }
    }
}

impl Error for DynamicTagError {}

/// Validates the dynamic-tag scaffolds of a parser projection, which wraps each expression in
/// generated parentheses, and returns the reports in parser-projection bytes.
#[cfg(feature = "parser")]
pub(crate) fn validate_dynamic_tags_with_synthetic_calls(
    program: &oxc_ast::ast::Program<'_>,
    contract: Option<DynamicTagContract<'_>>,
    synthetic_callee_spans: &[(u32, u32)],
) -> Result<Vec<InvalidDynamicTag>, DynamicTagError> {
    validate(program, contract, synthetic_callee_spans, true)
}

fn validate(
    program: &oxc_ast::ast::Program<'_>,
    contract: Option<DynamicTagContract<'_>>,
    synthetic_callee_spans: &[(u32, u32)],
    wrapped: bool,
) -> Result<Vec<InvalidDynamicTag>, DynamicTagError> {
    if synthetic_callee_spans.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DynamicTagError::UnorderedSyntheticCallees);
    }
    let Some(contract) = contract else {
        return Ok(Vec::new());
    };
    let count = usize::try_from(contract.count)
        .map_err(|_| DynamicTagError::CountExceedsAddressableMemory)?;
    if count == 0 || contract.prefix.is_empty() || contract.original_offsets.len() != count {
        return Err(DynamicTagError::EmptyContract);
    }
    let mut validator = DynamicTagValidator {
        prefix: contract.prefix,
        source: program.source_text,
        wrapped,
        seen: vec![false; count],
        invalid: Vec::new(),
        error: None,
    };
    validator.visit_program(program);
    if let Some(error) = validator.error {
        return Err(error);
    }
    if let Some(index) = validator.seen.iter().position(|seen| !seen) {
        return Err(DynamicTagError::LostScaffold { index });
    }
    Ok(validator.invalid)
}

struct DynamicTagValidator<'c> {
    prefix: &'c str,
    source: &'c str,
    /// Whether the projection wrapped each expression in generated parentheses.
    wrapped: bool,
    seen: Vec<bool>,
    invalid: Vec<InvalidDynamicTag>,
    error: Option<DynamicTagError>,
}

impl<'a> Visit<'a> for DynamicTagValidator<'_> {
    fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
        if self.error.is_some() {
            return;
        }
        let Some(name) = element.name.get_identifier_name() else {
            walk::walk_jsx_opening_element(self, element);
            return;
        };
        let Some(index) = scaffold_ordinal(name.as_str(), self.prefix, 'D', false) else {
            if name.as_str().starts_with(self.prefix) {
                self.error = Some(DynamicTagError::MalformedScaffold);
                return;
            }
            walk::walk_jsx_opening_element(self, element);
            return;
        };
        let index = index as usize;
        if index >= self.seen.len() || self.seen[index] {
            self.error = Some(DynamicTagError::InvalidScaffold { index });
            return;
        }

        let (container, expression) = match dynamic_tag_expression(element, self.prefix, index) {
            Ok(found) => found,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        // The parser projection writes the authored expression as `{(expression)}`. Those two
        // parentheses are generated, so only parentheses strictly inside them are authored. The
        // toolchain projection writes `{expression}`, so its parentheses are all authored.
        let bytes = self.source.as_bytes();
        let generated = self.wrapped
            && bytes.get(container.start as usize + 1) == Some(&b'(')
            && container.end >= 2
            && bytes.get(container.end as usize - 2) == Some(&b')');
        let inset = if generated { 2 } else { 1 };
        let authored = Span::new(
            container.start.saturating_add(inset),
            container.end.saturating_sub(inset).max(container.start.saturating_add(inset)),
        );
        let expression = match expression {
            Expression::ParenthesizedExpression(wrapper)
                if wrapper.span.start.saturating_add(1) == authored.start
                    && wrapper.span.end.saturating_sub(1) == authored.end =>
            {
                &wrapper.expression
            }
            expression => expression,
        };
        let parts = DynamicTagParts { source: self.source, authored };
        let invalid = parts.invalid_part(expression);
        self.seen[index] = true;
        // `@tsrx/core` checks an opening tag once its name is parsed, so a dynamic tag nested in
        // the name expression reports first. A nested tag in the closing name is a child here and
        // reports after, as it does there.
        walk::walk_jsx_opening_element(self, element);
        if let Some(span) = invalid {
            self.invalid.push(InvalidDynamicTag { index, start: span.start, end: span.end });
        }
    }
}

fn dynamic_tag_expression<'a, 'element>(
    element: &'element JSXOpeningElement<'a>,
    prefix: &str,
    index: usize,
) -> Result<(Span, &'element Expression<'a>), DynamicTagError> {
    let mut expression = None;
    let mut end_sentinel = false;
    for item in &element.attributes {
        let Some(attribute) = item.as_attribute() else {
            continue;
        };
        let Some(identifier) = attribute.name.as_identifier() else {
            continue;
        };
        let name = identifier.name.as_str();
        if let Some(attribute_index) = scaffold_ordinal(name, prefix, 'Z', true) {
            let valid_value = attribute.value.as_ref().is_some_and(|value| {
                matches!(
                    value,
                    JSXAttributeValue::ExpressionContainer(container)
                        if matches!(container.expression.as_expression(), Some(Expression::NullLiteral(_)))
                )
            });
            if attribute_index as usize != index || end_sentinel || !valid_value {
                return Err(DynamicTagError::MismatchedEndScaffold { index });
            }
            end_sentinel = true;
            continue;
        }
        // A shorthand attribute's scaffold name: `S` in the parser projection, `V` in the lint
        // and format projections.
        if scaffold_ordinal(name, prefix, 'S', true).is_some()
            || scaffold_ordinal(name, prefix, 'V', true).is_some()
        {
            continue;
        }
        let Some(attribute_index) = scaffold_ordinal(name, prefix, 'A', true) else {
            if name.starts_with(prefix) {
                return Err(DynamicTagError::MalformedAttribute { index });
            }
            continue;
        };
        if attribute_index as usize != index || expression.is_some() {
            return Err(DynamicTagError::MismatchedAttribute { index });
        }
        expression = attribute.value.as_ref().and_then(|value| match value {
            JSXAttributeValue::ExpressionContainer(container) => {
                container.expression.as_expression().map(|expression| (container.span, expression))
            }
            _ => None,
        });
    }
    let expression = expression.ok_or(DynamicTagError::MissingExpression { index })?;
    if !end_sentinel {
        return Err(DynamicTagError::MissingEndScaffold { index });
    }
    Ok(expression)
}

fn scaffold_ordinal(name: &str, prefix: &str, kind: char, suffix: bool) -> Option<u32> {
    let rest = name.strip_prefix(prefix)?.strip_prefix(kind)?;
    let digits = if suffix { rest.strip_suffix('_')? } else { rest };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `@tsrx/core`'s `find_invalid_dynamic_tag_part` over OXC's AST.
///
/// A dynamic tag expression is an identifier, a member access chain that starts at an identifier
/// or `this` and whose computed keys are identifiers, string or number literals, or member
/// accesses, or a string literal. The search follows only a member access's object and computed
/// key and returns the first part that isn't allowed, widened over the parentheses around it.
///
/// OXC may be asked not to keep parentheses as nodes, so a part is parenthesized when authored
/// parentheses enclose exactly its span. That reads only the source between the generated
/// parentheses the projection wraps the expression in.
struct DynamicTagParts<'s> {
    source: &'s str,
    authored: Span,
}

impl DynamicTagParts<'_> {
    fn invalid_part(&self, expression: &Expression<'_>) -> Option<Span> {
        if let Some(span) = self.parenthesized(expression) {
            return Some(span);
        }
        match expression {
            Expression::Identifier(identifier) => {
                (identifier.name == "undefined").then_some(identifier.span)
            }
            Expression::StringLiteral(_) => None,
            Expression::StaticMemberExpression(member) => self.invalid_object(&member.object),
            Expression::PrivateFieldExpression(member) => self.invalid_object(&member.object),
            Expression::ComputedMemberExpression(member) => {
                self.invalid_object(&member.object).or_else(|| self.invalid_key(&member.expression))
            }
            expression => Some(expression.span()),
        }
    }

    fn invalid_object(&self, object: &Expression<'_>) -> Option<Span> {
        match object {
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ComputedMemberExpression(_) => self.invalid_part(object),
            Expression::ThisExpression(_) => self.parenthesized(object),
            object => Some(self.widen(object.span())),
        }
    }

    fn invalid_key(&self, key: &Expression<'_>) -> Option<Span> {
        if let Some(span) = self.parenthesized(key) {
            return Some(span);
        }
        match key {
            Expression::Identifier(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_) => None,
            Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ComputedMemberExpression(_) => self.invalid_part(key),
            key => Some(key.span()),
        }
    }

    /// The span of `expression` and the parentheses around it, when there are any.
    fn parenthesized(&self, expression: &Expression<'_>) -> Option<Span> {
        let span = expression.span();
        let widened = self.widen(span);
        (widened != span || matches!(expression, Expression::ParenthesizedExpression(_)))
            .then_some(widened)
    }

    /// Widens `span` over every pair of authored parentheses that encloses exactly it.
    fn widen(&self, mut span: Span) -> Span {
        loop {
            let (Some(before), Some(after)) =
                (self.trivia_before(span.start), self.trivia_after(span.end))
            else {
                return span;
            };
            let bytes = self.source.as_bytes();
            let opens =
                before > self.authored.start && bytes.get(before as usize - 1) == Some(&b'(');
            let closes = after < self.authored.end && bytes.get(after as usize) == Some(&b')');
            if !(opens && closes) {
                return span;
            }
            span = Span::new(before - 1, after + 1);
        }
    }

    /// The offset before the whitespace and block comments that end at `offset`.
    fn trivia_before(&self, offset: u32) -> Option<u32> {
        let mut text = self.source.get(self.authored.start as usize..offset as usize)?;
        loop {
            let trimmed = text.trim_end();
            text = match trimmed.strip_suffix("*/").and_then(|inner| inner.rfind("/*")) {
                Some(comment_start) => &trimmed[..comment_start],
                None => {
                    return u32::try_from(self.authored.start as usize + trimmed.len()).ok();
                }
            };
        }
    }

    /// The offset after the whitespace and comments that start at `offset`.
    fn trivia_after(&self, offset: u32) -> Option<u32> {
        let text = self.source.get(offset as usize..self.authored.end as usize)?;
        let mut rest = text;
        loop {
            let trimmed = rest.trim_start();
            rest = if let Some(line) = trimmed.strip_prefix("//") {
                line.find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or("", |end| &line[end..])
            } else if let Some(block) = trimmed.strip_prefix("/*") {
                &block[block.find("*/")? + 2..]
            } else {
                let consumed = text.len() - trimmed.len();
                return u32::try_from(offset as usize + consumed).ok();
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DynamicTagError;

    #[test]
    fn every_variant_is_a_positionless_contract_defect() {
        // An authored tag expression is reported with its Program, never as one of these, so no
        // message may carry an authored position for an editor to scrape.
        let variants = [
            DynamicTagError::UnorderedSyntheticCallees,
            DynamicTagError::CountExceedsAddressableMemory,
            DynamicTagError::EmptyContract,
            DynamicTagError::MalformedScaffold,
            DynamicTagError::InvalidScaffold { index: 3 },
            DynamicTagError::LostScaffold { index: 3 },
            DynamicTagError::MismatchedEndScaffold { index: 3 },
            DynamicTagError::MalformedAttribute { index: 3 },
            DynamicTagError::MismatchedAttribute { index: 3 },
            DynamicTagError::MissingExpression { index: 3 },
            DynamicTagError::MissingEndScaffold { index: 3 },
        ];
        for error in &variants {
            assert!(!error.to_string().contains("byte "), "{error}");
        }
    }
}

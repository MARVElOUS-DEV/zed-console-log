use crate::{
    config::{InsertPosition, Settings, dotted_identifier},
    format::{self, Context, MARKER},
    text::{self, Edit},
};
use oxc::{
    allocator::Allocator,
    ast::{
        AstKind,
        ast::{Expression, FunctionBody, Statement},
    },
    parser::Parser,
    semantic::{AstNodes, Scoping, SemanticBuilder},
    span::{GetSpan, SourceType, Span},
    syntax::node::NodeId,
};

#[derive(Debug)]
pub struct Action {
    pub title: String,
    pub kind: String,
    pub preferred: bool,
    pub edits: Vec<Edit>,
}

pub fn source_type(language: &str) -> Option<SourceType> {
    Some(match language {
        "javascript" => SourceType::unambiguous().with_jsx(true),
        "javascriptreact" | "jsx" => SourceType::unambiguous().with_jsx(true),
        "typescript" => SourceType::ts(),
        "typescriptreact" | "tsx" => SourceType::tsx(),
        _ => return None,
    })
}

pub fn code_actions(
    uri: &str,
    language: &str,
    source: &str,
    selection: std::ops::Range<usize>,
    settings: &Settings,
    only: &[String],
) -> Vec<Action> {
    let Some(source_type) = source_type(language) else {
        return Vec::new();
    };
    if source.len() > u32::MAX as usize
        || selection.start > selection.end
        || source.get(selection.clone()).is_none()
    {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Vec::new();
    }
    let built = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.errors.is_empty() {
        return Vec::new();
    }
    let nodes = built.semantic.nodes();
    let scoping = built.semantic.scoping();
    let mut result = Vec::new();
    if let Some(id) = find_target(nodes, scoping, selection.clone())
        && let Some(plan) = insertion_plan(nodes, scoping, id, source, settings)
    {
        let span = nodes.kind(id).span();
        let expression = &source[span.start as usize..span.end as usize];
        if !expression.contains(['\r', '\n']) {
            let context = context_at(nodes, source, span.start);
            let mut callees: Vec<_> = settings
                .methods
                .iter()
                .map(|m| format!("console.{m}"))
                .collect();
            if let Some(custom) = &settings.custom_function {
                callees.push(custom.clone());
            }
            let mut seen = std::collections::HashSet::new();
            callees.retain(|callee| seen.insert(callee.clone()));
            for callee in callees {
                let mut style = settings.clone();
                // Inline insertion and continuation tokens require explicit ASI protection.
                if plan.inline
                    || source[plan.offset..]
                        .trim_start()
                        .starts_with(['(', '[', '/', '\u{60}', '+', '-'])
                {
                    style.semicolon = true;
                }
                let message = format::label(uri, plan.line, expression, &context, settings);
                let statement = format::statement(&callee, &message, expression, &style);
                let edit = Edit {
                    start: plan.offset,
                    end: plan.offset,
                    text: format!("{}{statement}{}", plan.prefix, plan.suffix),
                };
                if valid_after(source, std::slice::from_ref(&edit), source_type, None) {
                    result.push(Action {
                        title: format!("Insert {callee} for {expression}"),
                        kind: "refactor.rewrite.zedConsoleLog.insert".into(),
                        preferred: callee == "console.log",
                        edits: vec![edit],
                    });
                }
            }
        }
    }

    let mut owned = Vec::new();
    for comment in &parsed.program.comments {
        let span = comment.span;
        if &source[span.start as usize..span.end as usize] != MARKER {
            continue;
        }
        for node in nodes.iter() {
            let AstKind::ExpressionStatement(statement) = node.kind() else {
                continue;
            };
            let start = statement.span.start as usize;
            if start < span.end as usize
                || !source[span.end as usize..start].trim().is_empty()
                || !statement_container(nodes.parent_kind(node.id()))
            {
                continue;
            }
            if let Some((_callee, expression, label_span)) =
                owned_call(&statement.expression, source)
            {
                owned.push(Owned {
                    start: span.start as usize,
                    end: statement.span.end as usize,
                    expression,
                    label_span,
                    line: text::position(source, start).map_or(1, |p| p.line as usize + 1),
                    commented: false,
                    original: String::new(),
                });
            }
            break;
        }
    }

    // Reparse disabled statements; never trust a marker appearing in arbitrary comment text.
    let comments = &parsed.program.comments;
    for (index, comment) in comments.iter().enumerate() {
        let raw = &source[comment.span.start as usize..comment.span.end as usize];
        if !comment_container(nodes, comment.span.start)
            || !comment.is_line()
            || !raw
                .strip_prefix("//")
                .is_some_and(|s| s.trim_start().starts_with(MARKER))
        {
            continue;
        }
        if !source
            [text::line_start(source, comment.span.start as usize)..comment.span.start as usize]
            .trim()
            .is_empty()
        {
            continue;
        }
        let mut original = String::new();
        let mut previous_end = comment.span.start as usize;
        for next in comments.iter().skip(index) {
            if !next.is_line()
                || !source[previous_end..next.span.start as usize]
                    .trim()
                    .is_empty()
            {
                break;
            }
            let raw = &source[next.span.start as usize..next.span.end as usize];
            let part = raw.strip_prefix("//").unwrap_or(raw);
            if !original.is_empty() {
                original.push_str(text::newline(source));
            }
            original.push_str(part.strip_prefix(' ').unwrap_or(part));
            previous_end = next.span.end as usize;
            if let Some((_callee, expression, label_span, call_start)) =
                parse_owned(&original, source_type)
            {
                owned.push(Owned {
                    start: comment.span.start as usize,
                    end: next.span.end as usize,
                    expression,
                    label_span,
                    line: text::position(source, comment.span.start as usize)
                        .map_or(1, |p| p.line as usize + 1)
                        + original[..call_start]
                            .bytes()
                            .filter(|b| *b == b'\n')
                            .count(),
                    commented: true,
                    original: original.clone(),
                });
                break;
            }
        }
    }
    owned.sort_by_key(|log| log.start);
    owned.dedup_by_key(|log| log.start);
    owned.retain(|log| {
        selection.is_empty() || log.start < selection.end && log.end > selection.start
    });
    for operation in ["comment", "uncomment", "delete", "correct"] {
        let mut edits = Vec::new();
        for log in &owned {
            let replacement = match operation {
                "comment" if !log.commented && standalone(source, log.start, log.end) => Some(
                    source[log.start..log.end]
                        .split('\n')
                        .map(|line| format!("// {line}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                "uncomment" if log.commented => Some(log.original.clone()),
                "delete" => {
                    let start = text::line_start(source, log.start);
                    let end = text::line_end(source, log.end);
                    if source[start..log.start].trim().is_empty()
                        && source[log.end..end].trim().is_empty()
                    {
                        edits.push(Edit {
                            start,
                            end,
                            text: String::new(),
                        });
                    } else {
                        edits.push(Edit {
                            start: log.start,
                            end: log.end,
                            text: String::new(),
                        });
                    }
                    None
                }
                "correct" => {
                    let Some(span) = &log.label_span else {
                        continue;
                    };
                    let context = context_at(nodes, source, log.start as u32);
                    let message = format::label(uri, log.line, &log.expression, &context, settings);
                    let quoted = format::quote_string(&message, settings.quote);
                    if log.commented {
                        let mut original = log.original.clone();
                        original.replace_range(span.clone(), &quoted);
                        Some(
                            original
                                .split('\n')
                                .map(|line| format!("// {line}"))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        if quoted != source[span.clone()] {
                            edits.push(Edit {
                                start: span.start,
                                end: span.end,
                                text: quoted,
                            });
                        }
                        None
                    }
                }
                _ => None,
            };
            if let Some(replacement) = replacement
                && replacement != source[log.start..log.end]
            {
                edits.push(Edit {
                    start: log.start,
                    end: log.end,
                    text: replacement,
                });
            }
        }
        if !edits.is_empty() && valid_after(source, &edits, source_type, Some(nodes)) {
            result.push(Action {
                title: format!(
                    "{} generated logs ({})",
                    match operation {
                        "comment" => "Comment",
                        "uncomment" => "Uncomment",
                        "delete" => "Delete",
                        _ => "Correct",
                    },
                    if selection.is_empty() {
                        "document"
                    } else {
                        "selection"
                    }
                ),
                kind: format!("source.zedConsoleLog.{operation}"),
                preferred: false,
                edits,
            });
        }
    }
    result.retain(|action| {
        only.is_empty()
            || only.iter().any(|kind| {
                action.kind == *kind
                    || action
                        .kind
                        .strip_prefix(kind)
                        .is_some_and(|tail| tail.starts_with('.'))
            })
    });
    result
}

fn valid_after(
    source: &str,
    edits: &[Edit],
    source_type: SourceType,
    original_nodes: Option<&AstNodes<'_>>,
) -> bool {
    let Some(updated) = text::apply(source, edits) else {
        return false;
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &updated, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return false;
    }
    let Some(original_nodes) = original_nodes else {
        return true;
    };
    // A successful parse can still merge semicolonless application statements.
    // Withhold bulk actions unless every untouched statement keeps its boundaries.
    let built = SemanticBuilder::new().build(&parsed.program);
    let statements: std::collections::HashSet<_> = built
        .semantic
        .nodes()
        .iter()
        .filter(|node| node.kind().is_statement() || node.kind().is_declaration())
        .map(|node| (i64::from(node.span().start), i64::from(node.span().end)))
        .collect();
    original_nodes.iter().all(|node| {
        if !node.kind().is_statement() && !node.kind().is_declaration() {
            return true;
        }
        let span = node.span();
        let start = span.start as usize;
        let end = span.end as usize;
        if edits
            .iter()
            .any(|edit| edit.start < end && edit.end > start)
        {
            return true;
        }
        let shift: i64 = edits
            .iter()
            .filter(|edit| edit.end <= start)
            .map(|edit| edit.text.len() as i64 - (edit.end - edit.start) as i64)
            .sum();
        statements.contains(&(i64::from(span.start) + shift, i64::from(span.end) + shift))
    })
}

fn safe_expression(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_) | Expression::ThisExpression(_) => true,
        Expression::StaticMemberExpression(m) => !m.optional && safe_expression(&m.object),
        Expression::ComputedMemberExpression(m) => {
            !m.optional
                && safe_expression(&m.object)
                && matches!(
                    m.expression,
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                )
        }
        Expression::ParenthesizedExpression(v) => safe_expression(&v.expression),
        Expression::TSAsExpression(v) => safe_expression(&v.expression),
        Expression::TSSatisfiesExpression(v) => safe_expression(&v.expression),
        Expression::TSTypeAssertion(v) => safe_expression(&v.expression),
        Expression::TSNonNullExpression(v) => safe_expression(&v.expression),
        _ => false,
    }
}

fn safe_kind(kind: AstKind<'_>) -> bool {
    match kind {
        AstKind::IdentifierReference(_)
        | AstKind::BindingIdentifier(_)
        | AstKind::ThisExpression(_) => true,
        AstKind::StaticMemberExpression(m) => !m.optional && safe_expression(&m.object),
        AstKind::ComputedMemberExpression(m) => {
            !m.optional
                && safe_expression(&m.object)
                && matches!(
                    m.expression,
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                )
        }
        AstKind::ParenthesizedExpression(v) => safe_expression(&v.expression),
        AstKind::TSAsExpression(v) => safe_expression(&v.expression),
        AstKind::TSSatisfiesExpression(v) => safe_expression(&v.expression),
        AstKind::TSTypeAssertion(v) => safe_expression(&v.expression),
        AstKind::TSNonNullExpression(v) => safe_expression(&v.expression),
        _ => false,
    }
}

fn find_target(
    nodes: &AstNodes<'_>,
    scoping: &Scoping,
    selection: std::ops::Range<usize>,
) -> Option<NodeId> {
    let mut candidates: Vec<_> = nodes
        .iter()
        .filter(|node| {
            let span = node.span();
            let contains = if selection.is_empty() {
                span.start as usize <= selection.start && selection.start < span.end as usize
            } else {
                span.start as usize == selection.start && span.end as usize == selection.end
            };
            contains && safe_kind(node.kind())
        })
        .collect();
    candidates.sort_by_key(|node| node.span().size());
    let mut id = candidates.first()?.id();
    if selection.is_empty() {
        loop {
            let parent = nodes.parent_id(id);
            if matches!(
                nodes.kind(parent),
                AstKind::StaticMemberExpression(_) | AstKind::ComputedMemberExpression(_)
            ) && safe_kind(nodes.kind(parent))
            {
                id = parent;
            } else {
                break;
            }
        }
    }
    let binding = matches!(nodes.kind(id), AstKind::BindingIdentifier(_));
    for ancestor in nodes.ancestor_kinds(id) {
        // Unlike a direct read, typeof permits an unresolved identifier.
        if let AstKind::UnaryExpression(unary) = ancestor
            && unary.operator.is_typeof()
            && let Expression::Identifier(reference) = unary.argument.get_inner_expression()
            && reference.reference_id.get().is_none_or(|reference_id| {
                scoping.get_reference(reference_id).symbol_id().is_none()
            })
        {
            return None;
        }
        if (!binding
            && matches!(
                ancestor,
                AstKind::ChainExpression(_)
                    | AstKind::ConditionalExpression(_)
                    | AstKind::LogicalExpression(_)
                    | AstKind::AssignmentPattern(_)
            ))
            || ancestor.is_type()
            || matches!(
                ancestor,
                AstKind::ImportDeclaration(_)
                    | AstKind::ExportSpecifier(_)
                    | AstKind::JSXAttribute(_)
                    | AstKind::JSXOpeningElement(_)
                    | AstKind::TSEnumDeclaration(_)
                    | AstKind::Decorator(_)
            )
        {
            return None;
        }
    }
    if matches!(nodes.kind(id), AstKind::BindingIdentifier(_)) {
        let mut allowed = false;
        for kind in nodes.ancestor_kinds(id) {
            match kind {
                AstKind::VariableDeclarator(_)
                | AstKind::FormalParameter(_)
                | AstKind::FormalParameterRest(_)
                | AstKind::CatchParameter(_) => {
                    allowed = true;
                    break;
                }
                AstKind::Function(_) | AstKind::Class(_) | AstKind::ArrowFunctionExpression(_) => {
                    break;
                }
                _ => {}
            }
        }
        if !allowed {
            return None;
        }
    }
    Some(id)
}

struct Plan {
    offset: usize,
    prefix: String,
    suffix: String,
    line: usize,
    inline: bool,
}

fn before(source: &str, byte: usize) -> Plan {
    let start = text::line_start(source, byte);
    let inline = !source[start..byte].trim().is_empty();
    Plan {
        offset: if inline { byte } else { start },
        prefix: if inline {
            String::new()
        } else {
            source[start..byte].to_string()
        },
        suffix: if inline {
            " ".into()
        } else {
            text::newline(source).into()
        },
        line: text::position(source, byte).map_or(1, |p| p.line as usize + 1),
        inline,
    }
}

fn after(source: &str, span: Span) -> Plan {
    let byte = span.end as usize;
    let end = text::line_end(source, byte);
    let rest = source[byte..end].trim();
    let inline = !rest.is_empty() && !rest.starts_with("//");
    let at_eof = end == source.len() && !source.ends_with('\n');
    let offset = if inline { byte } else { end };
    let indent = text::indentation(source, span.start as usize);
    Plan {
        offset,
        prefix: if inline {
            " ".into()
        } else if at_eof {
            format!("{}{indent}", text::newline(source))
        } else {
            indent.into()
        },
        suffix: if inline {
            " ".into()
        } else if at_eof {
            String::new()
        } else {
            text::newline(source).into()
        },
        line: text::position(source, offset).map_or(1, |p| p.line as usize + 1)
            + usize::from(!inline && at_eof),
        inline,
    }
}

fn body_start(source: &str, body: &FunctionBody<'_>, settings: &Settings) -> Option<Plan> {
    if let Some(statement) = body.statements.first() {
        return Some(before(source, statement.span().start as usize));
    }
    if let Some(directive) = body.directives.last() {
        return Some(after(source, directive.span));
    }
    empty_block(source, body.span, settings)
}

fn empty_block(source: &str, span: Span, settings: &Settings) -> Option<Plan> {
    let open = span.start as usize + 1;
    let close = span.end as usize - 1;
    if !source.get(open..close)?.trim().is_empty() {
        return None;
    }
    let indent = text::indentation(source, span.start as usize);
    Some(Plan {
        offset: open,
        prefix: format!(
            "{}{indent}{}",
            text::newline(source),
            " ".repeat(settings.indent_size as usize)
        ),
        suffix: format!("{}{indent}", text::newline(source)),
        line: text::position(source, open)?.line as usize + 2,
        inline: false,
    })
}

fn block_start(
    source: &str,
    statement: &Statement<'_>,
    settings: &Settings,
    binding: AstKind<'_>,
    scoping: &Scoping,
) -> Option<Plan> {
    let Statement::BlockStatement(block) = statement else {
        return None;
    };
    let AstKind::BindingIdentifier(binding) = binding else {
        return None;
    };
    // The body has its own lexical scope, including declarations after the log.
    if scoping.find_binding(block.scope_id.get()?, binding.name) != Some(binding.symbol_id.get()?) {
        return None;
    }
    if let Some(statement) = block.body.first() {
        Some(before(source, statement.span().start as usize))
    } else {
        empty_block(source, block.span, settings)
    }
}

fn statement_container(kind: AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::Program(_)
            | AstKind::BlockStatement(_)
            | AstKind::FunctionBody(_)
            | AstKind::SwitchCase(_)
    )
}

fn insertion_plan(
    nodes: &AstNodes<'_>,
    scoping: &Scoping,
    id: NodeId,
    source: &str,
    settings: &Settings,
) -> Option<Plan> {
    let binding = matches!(nodes.kind(id), AstKind::BindingIdentifier(_));
    let mut in_parameters = false;
    let mut in_catch = false;
    for (ancestor_id, node) in nodes.ancestors_enumerated(id) {
        match node.kind() {
            AstKind::FormalParameters(_) => in_parameters = true,
            AstKind::CatchParameter(_) => in_catch = true,
            AstKind::Function(f) if in_parameters && binding => {
                return body_start(source, f.body.as_ref()?, settings);
            }
            AstKind::ArrowFunctionExpression(f) if in_parameters && binding => {
                return (!f.expression)
                    .then(|| body_start(source, &f.body, settings))
                    .flatten();
            }
            AstKind::CatchClause(catch) if in_catch && binding => {
                if let Some(statement) = catch.body.body.first() {
                    return Some(before(source, statement.span().start as usize));
                }
                return empty_block(source, catch.body.span, settings);
            }
            AstKind::VariableDeclaration(declaration) => match nodes.parent_kind(ancestor_id) {
                AstKind::ForStatement(l) if binding => {
                    return block_start(source, &l.body, settings, nodes.kind(id), scoping);
                }
                AstKind::ForInStatement(l) if binding => {
                    return block_start(source, &l.body, settings, nodes.kind(id), scoping);
                }
                AstKind::ForOfStatement(l) if binding => {
                    return block_start(source, &l.body, settings, nodes.kind(id), scoping);
                }
                parent if statement_container(parent) => {
                    return Some(after(source, declaration.span));
                }
                AstKind::ExportNamedDeclaration(export) => return Some(after(source, export.span)),
                _ => return None,
            },
            AstKind::ExpressionStatement(statement) => {
                if !statement_container(nodes.parent_kind(ancestor_id)) {
                    return None;
                }
                return Some(if settings.insert_position == InsertPosition::Above {
                    before(source, statement.span.start as usize)
                } else {
                    after(source, statement.span)
                });
            }
            AstKind::ReturnStatement(s) => {
                return statement_container(nodes.parent_kind(ancestor_id))
                    .then(|| before(source, s.span.start as usize));
            }
            AstKind::ThrowStatement(s) => {
                return statement_container(nodes.parent_kind(ancestor_id))
                    .then(|| before(source, s.span.start as usize));
            }
            AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::ClassBody(_)
            | AstKind::PropertyDefinition(_) => return None,
            _ => {}
        }
    }
    None
}

fn context_at(nodes: &AstNodes<'_>, source: &str, byte: u32) -> Context {
    let mut enclosing: Vec<_> = nodes
        .iter()
        .filter(|node| node.span().start <= byte && byte < node.span().end)
        .collect();
    enclosing.sort_by_key(|node| node.span().size());
    let mut context = Context::default();
    for node in enclosing {
        match node.kind() {
            AstKind::Class(class) if context.class_name.is_none() => {
                context.class_name = class.id.as_ref().map(|id| id.name.to_string());
            }
            AstKind::Function(f) if context.function_name.is_none() => {
                context.function_name =
                    f.id.as_ref()
                        .map(|id| id.name.to_string())
                        .or_else(|| function_owner(nodes, node.id(), source));
            }
            AstKind::ArrowFunctionExpression(_) if context.function_name.is_none() => {
                context.function_name = function_owner(nodes, node.id(), source);
            }
            _ => {}
        }
    }
    context
}

fn function_owner(nodes: &AstNodes<'_>, id: NodeId, source: &str) -> Option<String> {
    let span = match nodes.parent_kind(id) {
        AstKind::VariableDeclarator(v) => v.id.span(),
        AstKind::MethodDefinition(v) => v.key.span(),
        AstKind::ObjectProperty(v) => v.key.span(),
        _ => return None,
    };
    Some(source[span.start as usize..span.end as usize].to_string())
}

struct Owned {
    start: usize,
    end: usize,
    expression: String,
    commented: bool,
    original: String,
    label_span: Option<std::ops::Range<usize>>,
    line: usize,
}

type OwnedCall = (String, String, Option<std::ops::Range<usize>>);

fn owned_call(expression: &Expression<'_>, source: &str) -> Option<OwnedCall> {
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    if call.optional || call.type_arguments.is_some() {
        return None;
    }
    let span = call.callee.span();
    let callee = &source[span.start as usize..span.end as usize];
    if !dotted_identifier(callee) {
        return None;
    }
    let value = if callee == "console.table" {
        if call.arguments.len() != 1 {
            return None;
        }
        call.arguments.first()?.as_expression()?
    } else {
        if call.arguments.len() != 2
            || !matches!(
                call.arguments.first()?.as_expression()?,
                Expression::StringLiteral(_)
            )
        {
            return None;
        }
        call.arguments.get(1)?.as_expression()?
    };
    if !safe_expression(value) {
        return None;
    }
    let span = value.span();
    let label_span = if callee == "console.table" {
        None
    } else {
        let label = call.arguments.first()?.span();
        Some(label.start as usize..label.end as usize)
    };
    Some((
        callee.into(),
        source[span.start as usize..span.end as usize].into(),
        label_span,
    ))
}

type ParsedOwned = (String, String, Option<std::ops::Range<usize>>, usize);

fn parse_owned(source: &str, source_type: SourceType) -> Option<ParsedOwned> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked
        || !parsed.errors.is_empty()
        || parsed.program.body.len() != 1
        || !parsed.program.directives.is_empty()
        || parsed.program.comments.len() != 1
    {
        return None;
    }
    let comment = parsed.program.comments.first()?;
    if &source[comment.span.start as usize..comment.span.end as usize] != MARKER
        || !source[..comment.span.start as usize].trim().is_empty()
    {
        return None;
    }
    let Statement::ExpressionStatement(statement) = parsed.program.body.first()? else {
        return None;
    };
    if !source[comment.span.end as usize..statement.span.start as usize]
        .trim()
        .is_empty()
        || !source[statement.span.end as usize..].trim().is_empty()
    {
        return None;
    }
    let (callee, expression, label_span) = owned_call(&statement.expression, source)?;
    Some((
        callee,
        expression,
        label_span,
        statement.span.start as usize,
    ))
}

fn standalone(source: &str, start: usize, end: usize) -> bool {
    source[text::line_start(source, start)..start]
        .trim()
        .is_empty()
        && source[end..text::line_end(source, end)].trim().is_empty()
}

fn comment_container(nodes: &AstNodes<'_>, byte: u32) -> bool {
    nodes
        .iter()
        .filter(|node| node.span().start <= byte && byte < node.span().end)
        .min_by_key(|node| node.span().size())
        .is_some_and(|node| statement_container(node.kind()))
}

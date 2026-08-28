use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::command::{
    CommandArg, CommandLine, CommandName, CommandPipeline, Connector, ParsedCommand, RedirectKind,
    RedirectSpec, ShellAssignment, ShellScript, SimpleCommand,
};

/// Parse failures for shell input.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseError {
    Empty,
    UnterminatedQuote,
    UnterminatedEscape,
    MissingRedirectTarget,
    UnexpectedPipe,
}

/// Lightweight parser for shell command lines.
pub struct ShellParser;

impl ShellParser {
    pub const fn new() -> Self {
        Self
    }

    pub fn parse(&self, input: &str) -> Result<ParsedCommand, ParseError> {
        let script = self.parse_script(input)?;
        let Some(pipeline) = script.pipelines.first() else {
            return Err(ParseError::Empty);
        };
        let Some(command) = pipeline.commands.first() else {
            return Err(ParseError::Empty);
        };
        let Some(name) = command.name.as_ref() else {
            return Err(ParseError::Empty);
        };

        let mut line = CommandLine::new(name.word.clone());
        for arg in &command.args {
            line.args.push(arg.clone());
        }
        Ok(ParsedCommand::new(input.to_string(), line))
    }

    #[allow(unused_assignments)] // the finish_pipeline! macro reassigns builders
    pub fn parse_script(&self, input: &str) -> Result<ShellScript, ParseError> {
        let tokens = tokenize(input)?;
        if tokens.is_empty() {
            return Err(ParseError::Empty);
        }

        let mut script = ShellScript::new();
        let mut pipeline = CommandPipeline::new();
        let mut command = SimpleCommand::new();
        let mut saw_token = false;
        let mut index = 0usize;
        let mut pending_connector = Connector::Always;

        macro_rules! finish_pipeline {
            () => {
                if !command.is_empty() {
                    pipeline.commands.push(command);
                    command = SimpleCommand::new();
                }
                if !pipeline.commands.is_empty() {
                    pipeline.connector = pending_connector;
                    script.pipelines.push(pipeline);
                    pipeline = CommandPipeline::new();
                }
            };
        }

        while index < tokens.len() {
            match &tokens[index] {
                Token::Word(word) => {
                    saw_token = true;
                    if command.name.is_none() && command.args.is_empty() {
                        if let Some((name, value)) = parse_assignment(word) {
                            command.assignments.push(ShellAssignment { name, value });
                        } else {
                            command.name = Some(CommandName::new(word.word.clone()));
                        }
                    } else {
                        command.args.push(word.clone());
                    }
                }
                Token::Redirect(kind) => {
                    let Some(Token::Word(target)) = tokens.get(index + 1) else {
                        return Err(ParseError::MissingRedirectTarget);
                    };
                    saw_token = true;
                    command.redirects.push(RedirectSpec {
                        kind: *kind,
                        target: target.clone(),
                    });
                    index += 1;
                }
                Token::Pipe => {
                    if command.is_empty() {
                        return Err(ParseError::UnexpectedPipe);
                    }
                    saw_token = true;
                    pipeline.commands.push(command);
                    command = SimpleCommand::new();
                }
                Token::AndAnd | Token::OrOr => {
                    if command.is_empty() && pipeline.commands.is_empty() {
                        return Err(ParseError::UnexpectedPipe);
                    }
                    saw_token = true;
                    finish_pipeline!();
                    pending_connector = match &tokens[index] {
                        Token::AndAnd => Connector::And,
                        _ => Connector::Or,
                    };
                }
                Token::Separator => {
                    finish_pipeline!();
                    pending_connector = Connector::Always;
                }
            }
            index += 1;
        }

        finish_pipeline!();

        if !saw_token || script.pipelines.is_empty() {
            return Err(ParseError::Empty);
        }

        Ok(script)
    }
}

impl Default for ShellParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Token {
    Word(CommandArg),
    Redirect(RedirectKind),
    Pipe,
    AndAnd,
    OrOr,
    Separator,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum QuoteState {
    None,
    Single,
    Double,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = QuoteState::None;
    let mut escaped = false;
    let mut quoted = false;
    let mut expand = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match quote {
            QuoteState::Single => {
                if ch == '\'' {
                    quote = QuoteState::None;
                    quoted = true;
                } else {
                    current.push(ch);
                }
                continue;
            }
            QuoteState::Double => {
                match ch {
                    '"' => {
                        quote = QuoteState::None;
                        quoted = true;
                    }
                    '\\' => escaped = true,
                    _ => {
                        current.push(ch);
                        expand = true;
                    }
                }
                continue;
            }
            QuoteState::None => {}
        }

        match ch {
            '$' if matches!(chars.clone().next(), Some('(')) => {
                // `$( … )` and `$(( … ))` stay ONE word (spaces, `<`, `>`
                // inside belong to the expansion): consume through the
                // balanced closing paren(s) into the current word.
                current.push('$');
                let _ = chars.next(); // '('
                current.push('(');
                if matches!(chars.peek(), Some('(')) {
                    // Arithmetic: close on the matching `))`.
                    let _ = chars.next();
                    current.push('(');
                    let mut depth = 0i32;
                    let mut closed = false;
                    while let Some(next) = chars.next() {
                        match next {
                            '(' => depth += 1,
                            ')' => {
                                if depth == 0 {
                                    if matches!(chars.peek(), Some(')')) {
                                        let _ = chars.next();
                                        current.push_str("))");
                                        closed = true;
                                        break;
                                    }
                                    return Err(ParseError::UnterminatedQuote);
                                }
                                depth -= 1;
                            }
                            _ => {}
                        }
                        current.push(next);
                    }
                    if !closed {
                        return Err(ParseError::UnterminatedQuote);
                    }
                } else {
                    // Command substitution: close on the balanced `)`.
                    let mut depth = 1i32;
                    while let Some(next) = chars.next() {
                        match next {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    current.push(')');
                                    break;
                                }
                                current.push('(');
                                continue;
                            }
                            _ => {}
                        }
                        current.push(next);
                    }
                }
                expand = true;
            }
            '\\' => escaped = true,
            '\'' => {
                quote = QuoteState::Single;
                quoted = true;
            }
            '"' => {
                quote = QuoteState::Double;
                quoted = true;
                expand = true;
            }
            ' ' | '\t' | '\r' => flush_word(&mut tokens, &mut current, &mut quoted, &mut expand),
            '\n' | ';' => {
                flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
                tokens.push(Token::Separator);
            }
            '|' => {
                flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
                if matches!(chars.peek(), Some('|')) {
                    let _ = chars.next();
                    tokens.push(Token::OrOr);
                } else {
                    tokens.push(Token::Pipe);
                }
            }
            '&' => {
                flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
                if matches!(chars.peek(), Some('&')) {
                    let _ = chars.next();
                    tokens.push(Token::AndAnd);
                } else {
                    // Single `&` (background) is not supported: treat it as
                    // a statement separator so scripts degrade sanely.
                    tokens.push(Token::Separator);
                }
            }
            '<' => {
                flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
                tokens.push(Token::Redirect(RedirectKind::Input));
            }
            '>' => {
                flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
                if matches!(chars.peek(), Some('>')) {
                    let _ = chars.next();
                    tokens.push(Token::Redirect(RedirectKind::Append));
                } else {
                    tokens.push(Token::Redirect(RedirectKind::Output));
                }
            }
            '#' if current.is_empty() => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        tokens.push(Token::Separator);
                        break;
                    }
                }
            }
            _ => {
                current.push(ch);
                expand = true;
            }
        }
    }

    if escaped {
        return Err(ParseError::UnterminatedEscape);
    }
    if quote != QuoteState::None {
        return Err(ParseError::UnterminatedQuote);
    }

    flush_word(&mut tokens, &mut current, &mut quoted, &mut expand);
    Ok(tokens)
}

fn flush_word(
    tokens: &mut Vec<Token>,
    current: &mut String,
    quoted: &mut bool,
    expand: &mut bool,
) {
    if current.is_empty() && !*quoted {
        return;
    }
    tokens.push(Token::Word(CommandArg::new(
        core::mem::take(current),
        *quoted,
        *expand,
    )));
    *quoted = false;
    *expand = false;
}

fn parse_assignment(word: &CommandArg) -> Option<(String, CommandArg)> {
    let (name, value) = word.word.split_once('=')?;
    if !is_valid_identifier(name) {
        return None;
    }
    Some((
        name.to_string(),
        CommandArg::new(value.to_string(), word.quoted, word.expand),
    ))
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

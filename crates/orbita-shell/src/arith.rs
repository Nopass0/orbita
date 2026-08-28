//! Arithmetic expansion `$(( … ))` for the Orbita scripting language.
//!
//! Pure recursive-descent evaluator over `i64` with `$VAR` / `${VAR}` /
//! bare-identifier resolution through a caller-supplied lookup (missing
//! or non-numeric variables evaluate to 0, like `sh`). Grammar:
//!
//! ```text
//! expr   := cmp
//! cmp    := add ( ('<'|'<='|'>'|'>='|'=='|'!=') add )?
//! add    := mul ( ('+'|'-') mul )*
//! mul    := unary ( ('*'|'/'|'%') unary )*
//! unary  := '-' unary | primary
//! primary:= number | '(' expr ')' | ident | '$' ident | '${' ident '}'
//! ```

/// Evaluates `expr`; `lookup` resolves identifiers to values
/// (non-numeric/missing → 0). `None` on syntax errors or division by
/// zero (the shell then leaves the expansion unreplaced).
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub fn eval<F>(expr: &str, lookup: F) -> Option<i64>
where
    F: Fn(&str) -> Option<i64>,
{
    let tokens = tokenize(expr)?;
    let mut parser = Parser { tokens, at: 0, lookup };
    let value = parser.expr()?;
    if parser.at != parser.tokens.len() {
        return None; // trailing garbage
    }
    Some(value)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    LParen,
    RParen,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

fn tokenize(input: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0usize;
    while at < chars.len() {
        let ch = chars[at];
        match ch {
            ' ' | '\t' => at += 1,
            '(' => {
                tokens.push(Tok::LParen);
                at += 1;
            }
            ')' => {
                tokens.push(Tok::RParen);
                at += 1;
            }
            '+' => {
                tokens.push(Tok::Add);
                at += 1;
            }
            '-' => {
                tokens.push(Tok::Sub);
                at += 1;
            }
            '*' => {
                tokens.push(Tok::Mul);
                at += 1;
            }
            '/' => {
                tokens.push(Tok::Div);
                at += 1;
            }
            '%' => {
                tokens.push(Tok::Rem);
                at += 1;
            }
            '<' | '>' | '=' | '!' => {
                let two = at + 1 < chars.len() && chars[at + 1] == '=';
                let tok = match (ch, two) {
                    ('<', true) => Tok::Le,
                    ('<', false) => Tok::Lt,
                    ('>', true) => Tok::Ge,
                    ('>', false) => Tok::Gt,
                    ('=', true) => Tok::Eq,
                    ('!', true) => Tok::Ne,
                    _ => return None,
                };
                tokens.push(tok);
                at += if two { 2 } else { 1 };
            }
            '$' => {
                at += 1;
                if at < chars.len() && chars[at] == '{' {
                    let mut end = at + 1;
                    while end < chars.len() && chars[end] != '}' {
                        end += 1;
                    }
                    if end >= chars.len() {
                        return None;
                    }
                    let name: String = chars[at + 1..end].iter().collect();
                    tokens.push(Tok::Ident(name));
                    at = end + 1;
                } else {
                    let start = at;
                    while at < chars.len() && (chars[at] == '_' || chars[at].is_ascii_alphanumeric()) {
                        at += 1;
                    }
                    if at == start {
                        return None; // lone '$'
                    }
                    tokens.push(Tok::Ident(chars[start..at].iter().collect()));
                }
            }
            c if c.is_ascii_digit() => {
                let start = at;
                while at < chars.len() && chars[at].is_ascii_digit() {
                    at += 1;
                }
                let text: String = chars[start..at].iter().collect();
                tokens.push(Tok::Num(text.parse().ok()?));
            }
            c if c == '_' || c.is_ascii_alphabetic() => {
                let start = at;
                while at < chars.len() && (chars[at] == '_' || chars[at].is_ascii_alphanumeric()) {
                    at += 1;
                }
                tokens.push(Tok::Ident(chars[start..at].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

struct Parser<F> {
    tokens: Vec<Tok>,
    at: usize,
    lookup: F,
}

impl<F: Fn(&str) -> Option<i64>> Parser<F> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at)
    }

    fn expr(&mut self) -> Option<i64> {
        self.cmp()
    }

    fn cmp(&mut self) -> Option<i64> {
        let lhs = self.add()?;
        let op = match self.peek() {
            Some(Tok::Lt) => '<',
            Some(Tok::Le) => 'l',
            Some(Tok::Gt) => 'g',
            Some(Tok::Ge) => 'G',
            Some(Tok::Eq) => '=',
            Some(Tok::Ne) => '!',
            _ => return Some(lhs),
        };
        self.at += 1;
        let rhs = self.add()?;
        Some(match op {
            '<' => (lhs < rhs) as i64,
            'l' => (lhs <= rhs) as i64,
            'g' => (lhs > rhs) as i64,
            'G' => (lhs >= rhs) as i64,
            '=' => (lhs == rhs) as i64,
            _ => (lhs != rhs) as i64,
        })
    }

    fn add(&mut self) -> Option<i64> {
        let mut value = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Add) => true,
                Some(Tok::Sub) => false,
                _ => return Some(value),
            };
            self.at += 1;
            let rhs = self.mul()?;
            value = if op {
                value.wrapping_add(rhs)
            } else {
                value.wrapping_sub(rhs)
            };
        }
    }

    fn mul(&mut self) -> Option<i64> {
        let mut value = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Mul) => 0,
                Some(Tok::Div) => 1,
                Some(Tok::Rem) => 2,
                _ => return Some(value),
            };
            self.at += 1;
            let rhs = self.unary()?;
            value = match op {
                0 => value.wrapping_mul(rhs),
                1 => {
                    if rhs == 0 {
                        return None;
                    }
                    value.wrapping_div(rhs)
                }
                _ => {
                    if rhs == 0 {
                        return None;
                    }
                    value.wrapping_rem(rhs)
                }
            };
        }
    }

    fn unary(&mut self) -> Option<i64> {
        if matches!(self.peek(), Some(Tok::Sub)) {
            self.at += 1;
            return Some(-self.unary()?);
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<i64> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.at += 1;
                Some(n)
            }
            Some(Tok::Ident(name)) => {
                self.at += 1;
                Some((self.lookup)(&name).unwrap_or(0))
            }
            Some(Tok::LParen) => {
                self.at += 1;
                let value = self.expr()?;
                if matches!(self.peek(), Some(Tok::RParen)) {
                    self.at += 1;
                    Some(value)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, i64)]) -> impl Fn(&str) -> Option<i64> + 'a {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| *value)
        }
    }

    #[test]
    fn precedence_and_parens() {
        assert_eq!(eval("1 + 2 * 3", |_| None), Some(7));
        assert_eq!(eval("(1 + 2) * 3", |_| None), Some(9));
        assert_eq!(eval("2 * (3 + 4) - 5", |_| None), Some(9));
    }

    #[test]
    fn division_and_remainder() {
        assert_eq!(eval("7 / 2", |_| None), Some(3));
        assert_eq!(eval("7 % 3", |_| None), Some(1));
        assert_eq!(eval("1 / 0", |_| None), None);
        assert_eq!(eval("1 % 0", |_| None), None);
    }

    #[test]
    fn unary_minus_chain() {
        assert_eq!(eval("-5", |_| None), Some(-5));
        assert_eq!(eval("--5", |_| None), Some(5));
        assert_eq!(eval("3 * -2", |_| None), Some(-6));
    }

    #[test]
    fn variables_resolve() {
        assert_eq!(eval("$count + 1", vars(&[("count", 41)])), Some(42));
        assert_eq!(eval("${left} * ${right}", vars(&[("left", 6), ("right", 7)])), Some(42));
        assert_eq!(eval("missing + 1", |_| None), Some(1)); // missing → 0
    }

    #[test]
    fn comparisons_return_bool() {
        assert_eq!(eval("3 < 4", |_| None), Some(1));
        assert_eq!(eval("3 >= 4", |_| None), Some(0));
        assert_eq!(eval("2 == 2", |_| None), Some(1));
        assert_eq!(eval("2 != 2", |_| None), Some(0));
    }

    #[test]
    fn syntax_errors_rejected() {
        assert_eq!(eval("1 +", |_| None), None);
        assert_eq!(eval("(1", |_| None), None);
        assert_eq!(eval("1)", |_| None), None);
        assert_eq!(eval("", |_| None), None);
        assert_eq!(eval("3 ~ 4", |_| None), None);
    }

    #[test]
    fn whitespace_insensitive() {
        assert_eq!(eval("  12*3  ", |_| None), Some(36));
        assert_eq!(eval("1+2", |_| None), Some(3));
    }
}

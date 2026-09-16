use crate::CompileError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Fn,
    Extern,
    Export,
    Let,
    Return,
    If,
    Else,
    For,
    Import,
    From,
    As,
    True,
    False,
    Identifier(String),
    Integer(i128, Option<String>),
    String(String),
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    Semicolon,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn lex(source: &str) -> Result<Vec<Token>, CompileError> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();
    let (mut line, mut column) = (1, 1);

    while let Some(character) = chars.peek().copied() {
        let span = Span { line, column };
        match character {
            ' ' | '\t' | '\r' => {
                chars.next();
                column += 1;
            }
            '\n' => {
                chars.next();
                line += 1;
                column = 1;
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut word = String::new();
                while let Some(c @ ('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) = chars.peek() {
                    word.push(*c);
                    chars.next();
                    column += 1;
                }
                let kind = match word.as_str() {
                    "fn" => TokenKind::Fn,
                    "extern" => TokenKind::Extern,
                    "export" => TokenKind::Export,
                    "let" => TokenKind::Let,
                    "return" => TokenKind::Return,
                    "if" => TokenKind::If,
                    "else" => TokenKind::Else,
                    "for" => TokenKind::For,
                    "import" => TokenKind::Import,
                    "from" => TokenKind::From,
                    "as" => TokenKind::As,
                    "true" => TokenKind::True,
                    "false" => TokenKind::False,
                    _ => TokenKind::Identifier(word),
                };
                tokens.push(Token { kind, span });
            }
            '0'..='9' => {
                let mut number = String::new();
                while let Some(c @ '0'..='9') = chars.peek() {
                    number.push(*c);
                    chars.next();
                    column += 1;
                }
                let value = number.parse().map_err(|_| {
                    CompileError::new(span.line, span.column, "integer literal is too large")
                })?;
                let mut suffix = String::new();
                while let Some(c @ ('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) = chars.peek() {
                    suffix.push(*c);
                    chars.next();
                    column += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Integer(
                        value,
                        if suffix.is_empty() {
                            None
                        } else {
                            Some(suffix)
                        },
                    ),
                    span,
                });
            }
            '"' => {
                chars.next();
                column += 1;
                let mut value = String::new();
                loop {
                    match chars.next() {
                        Some('"') => {
                            column += 1;
                            break;
                        }
                        Some('\n') | None => {
                            return Err(CompileError::new(
                                span.line,
                                span.column,
                                "unterminated string literal",
                            ));
                        }
                        Some(character) => {
                            value.push(character);
                            column += 1;
                        }
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::String(value),
                    span,
                });
            }
            '-' => {
                chars.next();
                column += 1;
                tokens.push(Token {
                    kind: TokenKind::Minus,
                    span,
                });
            }
            '(' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::LeftParen,
                span,
            ),
            ')' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::RightParen,
                span,
            ),
            '{' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::LeftBrace,
                span,
            ),
            '}' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::RightBrace,
                span,
            ),
            '[' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::LeftBracket,
                span,
            ),
            ']' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::RightBracket,
                span,
            ),
            ':' => push_single(&mut tokens, &mut chars, &mut column, TokenKind::Colon, span),
            ',' => push_single(&mut tokens, &mut chars, &mut column, TokenKind::Comma, span),
            ';' => push_single(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::Semicolon,
                span,
            ),
            '=' => push_optional_equal(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::Equal,
                TokenKind::EqualEqual,
                span,
            ),
            '!' => {
                chars.next();
                column += 1;
                if chars.next_if_eq(&'=').is_none() {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        "expected `=` after `!`",
                    ));
                }
                column += 1;
                tokens.push(Token {
                    kind: TokenKind::BangEqual,
                    span,
                });
            }
            '<' => push_optional_equal(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::Less,
                TokenKind::LessEqual,
                span,
            ),
            '>' => push_optional_equal(
                &mut tokens,
                &mut chars,
                &mut column,
                TokenKind::Greater,
                TokenKind::GreaterEqual,
                span,
            ),
            '+' => push_single(&mut tokens, &mut chars, &mut column, TokenKind::Plus, span),
            '*' => push_single(&mut tokens, &mut chars, &mut column, TokenKind::Star, span),
            '/' => push_single(&mut tokens, &mut chars, &mut column, TokenKind::Slash, span),
            _ => {
                return Err(CompileError::new(
                    span.line,
                    span.column,
                    format!("unexpected character `{character}`"),
                ));
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { line, column },
    });
    Ok(tokens)
}

fn push_single(
    tokens: &mut Vec<Token>,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    column: &mut usize,
    kind: TokenKind,
    span: Span,
) {
    chars.next();
    *column += 1;
    tokens.push(Token { kind, span });
}

fn push_optional_equal(
    tokens: &mut Vec<Token>,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    column: &mut usize,
    single: TokenKind,
    with_equal: TokenKind,
    span: Span,
) {
    chars.next();
    *column += 1;
    let kind = if chars.next_if_eq(&'=').is_some() {
        *column += 1;
        with_equal
    } else {
        single
    };
    tokens.push(Token { kind, span });
}

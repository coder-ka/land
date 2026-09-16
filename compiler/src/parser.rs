use std::collections::HashSet;

use crate::CompileError;
use crate::ast::{
    BinaryOperator, Expression, ExpressionKind, Function, Import, ImportName, Parameter, Pattern,
    PatternKind, Program, Statement, Type, UnaryOperator,
};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>) -> Result<Program, CompileError> {
    let declared_functions = tokens
        .windows(2)
        .filter_map(|tokens| match (&tokens[0].kind, &tokens[1].kind) {
            (TokenKind::Fn, TokenKind::Identifier(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    Parser {
        tokens,
        cursor: 0,
        type_parameters: Vec::new(),
        nesting_depth: 0,
        declared_functions,
    }
    .parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    type_parameters: Vec<String>,
    nesting_depth: usize,
    declared_functions: HashSet<String>,
}

const MAX_NESTING_DEPTH: usize = 128;

impl Parser {
    fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut imports = Vec::new();
        while self.current().kind == TokenKind::Import {
            imports.push(self.parse_import()?);
        }
        let mut functions = Vec::new();
        while self.current().kind != TokenKind::Eof {
            if self.current().kind == TokenKind::Import {
                return self.error("imports must appear before function declarations");
            }
            functions.push(self.parse_function()?);
        }
        Ok(Program { imports, functions })
    }

    fn parse_import(&mut self) -> Result<Import, CompileError> {
        let span = self.expect_simple(TokenKind::Import)?.span;
        self.expect_simple(TokenKind::LeftBrace)?;
        let mut names = Vec::new();
        loop {
            let imported = self.expect_identifier()?;
            let local = if self.current().kind == TokenKind::As {
                self.advance();
                self.expect_identifier()?
            } else {
                imported.clone()
            };
            if names.iter().any(|name: &ImportName| name.local == local) {
                return Err(CompileError::new(
                    span.line,
                    span.column,
                    format!("duplicate import name `{local}`"),
                ));
            }
            names.push(ImportName { imported, local });
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        self.expect_simple(TokenKind::RightBrace)?;
        self.expect_simple(TokenKind::From)?;
        let module = self.advance();
        let TokenKind::String(module) = module.kind else {
            return Err(CompileError::new(
                module.span.line,
                module.span.column,
                "expected module path string",
            ));
        };
        self.expect_simple(TokenKind::Semicolon)?;
        self.declared_functions
            .extend(names.iter().map(|name| name.local.clone()));
        Ok(Import {
            names,
            module,
            span,
        })
    }

    fn parse_function(&mut self) -> Result<Function, CompileError> {
        let exported = if self.current().kind == TokenKind::Export {
            self.advance();
            true
        } else {
            false
        };
        let external = if self.current().kind == TokenKind::Extern {
            self.advance();
            let abi = self.advance();
            match abi.kind {
                TokenKind::String(name) if name == "C" => {}
                _ => {
                    return Err(CompileError::new(
                        abi.span.line,
                        abi.span.column,
                        "expected ABI string `\"C\"`",
                    ));
                }
            }
            true
        } else {
            false
        };
        let span = self.expect_simple(TokenKind::Fn)?.span;
        let name = self.expect_identifier()?;
        let type_parameters = if self.current().kind == TokenKind::Less {
            self.parse_type_parameters()?
        } else {
            Vec::new()
        };
        self.type_parameters.clone_from(&type_parameters);
        self.expect_simple(TokenKind::LeftParen)?;
        let parameters = self.parse_parameters()?;
        self.expect_simple(TokenKind::RightParen)?;
        self.expect_simple(TokenKind::Colon)?;
        let return_type = self.parse_type()?;
        let body = if external {
            self.expect_simple(TokenKind::Semicolon)?;
            None
        } else {
            Some(self.parse_block()?)
        };
        self.type_parameters.clear();

        Ok(Function {
            name,
            exported: exported || external,
            visible_functions: None,
            type_parameters,
            parameters,
            return_type,
            body,
            span,
        })
    }

    fn parse_type_parameters(&mut self) -> Result<Vec<String>, CompileError> {
        self.expect_simple(TokenKind::Less)?;
        let mut parameters = Vec::new();
        loop {
            let parameter = self.expect_identifier()?;
            if parameters.contains(&parameter) {
                return self.error(&format!("duplicate type parameter `{parameter}`"));
            }
            parameters.push(parameter);
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        self.expect_simple(TokenKind::Greater)?;
        Ok(parameters)
    }

    fn parse_parameters(&mut self) -> Result<Vec<Parameter>, CompileError> {
        let mut parameters = Vec::new();
        while self.current().kind != TokenKind::RightParen {
            let span = self.current().span;
            let name = self.expect_identifier()?;
            self.expect_simple(TokenKind::Colon)?;
            let parameter_type = self.parse_type()?;
            parameters.push(Parameter {
                name,
                parameter_type,
                span,
            });

            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        Ok(parameters)
    }

    fn parse_statement(&mut self) -> Result<Statement, CompileError> {
        match self.current().kind {
            TokenKind::Let => {
                let span = self.advance().span;
                let pattern = self.parse_pattern()?;
                let annotation = if self.current().kind == TokenKind::Colon {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect_simple(TokenKind::Equal)?;
                let value = self.parse_expression(0)?;
                self.expect_simple(TokenKind::Semicolon)?;
                Ok(Statement::Let {
                    pattern,
                    annotation,
                    value,
                    span,
                })
            }
            TokenKind::Identifier(_) if self.next().kind == TokenKind::Equal => {
                let token = self.advance();
                let TokenKind::Identifier(name) = token.kind else {
                    unreachable!();
                };
                self.advance();
                let value = self.parse_expression(0)?;
                self.expect_simple(TokenKind::Semicolon)?;
                Ok(Statement::Assign {
                    name,
                    value,
                    span: token.span,
                })
            }
            TokenKind::Return => {
                let span = self.advance().span;
                let expression = if self.current().kind == TokenKind::Semicolon {
                    None
                } else {
                    Some(self.parse_expression(0)?)
                };
                self.expect_simple(TokenKind::Semicolon)?;
                Ok(Statement::Return {
                    value: expression,
                    span,
                })
            }
            TokenKind::If => {
                let span = self.advance().span;
                let condition = self.parse_expression(0)?;
                let then_body = self.parse_block()?;
                let else_body = if self.current().kind == TokenKind::Else {
                    self.advance();
                    Some(self.parse_block()?)
                } else {
                    None
                };
                Ok(Statement::If {
                    condition,
                    then_body,
                    else_body,
                    span,
                })
            }
            TokenKind::For => {
                let span = self.advance().span;
                let condition = self.parse_expression(0)?;
                let body = self.parse_block()?;
                Ok(Statement::For {
                    condition,
                    body,
                    span,
                })
            }
            _ => {
                let expression = self.parse_expression(0)?;
                self.expect_simple(TokenKind::Semicolon)?;
                Ok(Statement::Expression(expression))
            }
        }
    }

    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        self.with_nesting(|parser| parser.parse_pattern_inner())
    }

    fn parse_pattern_inner(&mut self) -> Result<Pattern, CompileError> {
        let token = self.advance();
        let kind = match token.kind {
            TokenKind::Identifier(name) => PatternKind::Name(name),
            TokenKind::LeftParen => {
                let mut patterns = vec![self.parse_pattern()?];
                while self.current().kind == TokenKind::Comma {
                    self.advance();
                    if self.current().kind == TokenKind::RightParen {
                        break;
                    }
                    patterns.push(self.parse_pattern()?);
                }
                self.expect_simple(TokenKind::RightParen)?;
                PatternKind::Tuple(patterns)
            }
            _ => {
                return Err(CompileError::new(
                    token.span.line,
                    token.span.column,
                    "expected a binding pattern",
                ));
            }
        };
        Ok(Pattern {
            kind,
            span: token.span,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Statement>, CompileError> {
        self.with_nesting(|parser| parser.parse_block_inner())
    }

    fn parse_block_inner(&mut self) -> Result<Vec<Statement>, CompileError> {
        self.expect_simple(TokenKind::LeftBrace)?;
        let mut statements = Vec::new();
        while self.current().kind != TokenKind::RightBrace {
            if self.current().kind == TokenKind::Eof {
                return self.error("expected `}` before end of file");
            }
            statements.push(self.parse_statement()?);
        }
        self.advance();
        Ok(statements)
    }

    fn parse_expression(&mut self, minimum_precedence: u8) -> Result<Expression, CompileError> {
        let mut left = self.parse_primary()?;
        loop {
            let Some((operator, precedence)) = binary_operator(&self.current().kind) else {
                break;
            };
            if precedence < minimum_precedence {
                break;
            }
            self.advance();
            let right = self.parse_expression(precedence + 1)?;
            let span = left.span;
            left = self.expression(
                span,
                ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            );
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expression, CompileError> {
        self.with_nesting(|parser| parser.parse_primary_inner())
    }

    fn parse_primary_inner(&mut self) -> Result<Expression, CompileError> {
        let token = self.advance();
        let mut type_arguments = Vec::new();
        let kind = match token.kind {
            TokenKind::Integer(value, suffix) => ExpressionKind::Integer { value, suffix },
            TokenKind::True => ExpressionKind::Boolean(true),
            TokenKind::False => ExpressionKind::Boolean(false),
            TokenKind::Minus => ExpressionKind::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(self.parse_primary()?),
            },
            TokenKind::Identifier(name) => {
                if self.declared_functions.contains(&name) {
                    type_arguments = self.try_parse_type_arguments()?;
                }
                ExpressionKind::Name(name)
            }
            TokenKind::LeftBracket => {
                let values = self.parse_array_values()?;
                self.expect_simple(TokenKind::RightBracket)?;
                ExpressionKind::Array(values)
            }
            TokenKind::LeftParen => {
                let first = self.parse_expression(0)?;
                if self.current().kind == TokenKind::Comma {
                    let mut values = vec![first];
                    while self.current().kind == TokenKind::Comma {
                        self.advance();
                        if self.current().kind == TokenKind::RightParen {
                            break;
                        }
                        values.push(self.parse_expression(0)?);
                    }
                    self.expect_simple(TokenKind::RightParen)?;
                    ExpressionKind::Tuple(values)
                } else {
                    self.expect_simple(TokenKind::RightParen)?;
                    return self.parse_postfix(first, type_arguments);
                }
            }
            _ => {
                return Err(CompileError::new(
                    token.span.line,
                    token.span.column,
                    "expected an expression",
                ));
            }
        };
        let expression = self.expression(token.span, kind);
        self.parse_postfix(expression, type_arguments)
    }

    fn parse_postfix(
        &mut self,
        mut expression: Expression,
        mut type_arguments: Vec<Type>,
    ) -> Result<Expression, CompileError> {
        if !type_arguments.is_empty() && self.current().kind != TokenKind::LeftParen {
            return self.error("generic type arguments must be followed by a call");
        }
        loop {
            let span = expression.span;
            if self.current().kind == TokenKind::LeftBracket {
                self.advance();
                let index = self.parse_expression(0)?;
                self.expect_simple(TokenKind::RightBracket)?;
                expression = self.expression(
                    span,
                    ExpressionKind::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    },
                );
            } else if self.current().kind == TokenKind::LeftParen {
                self.advance();
                let arguments = self.parse_arguments()?;
                self.expect_simple(TokenKind::RightParen)?;
                expression = self.expression(
                    span,
                    ExpressionKind::Call {
                        callee: Box::new(expression),
                        type_arguments: std::mem::take(&mut type_arguments),
                        arguments,
                    },
                );
            } else {
                break;
            }
        }
        Ok(expression)
    }

    // Only declared function names reach this speculative type-argument parser.
    fn try_parse_type_arguments(&mut self) -> Result<Vec<Type>, CompileError> {
        if self.current().kind != TokenKind::Less {
            return Ok(Vec::new());
        }
        let saved = self.cursor;
        self.advance();
        let mut arguments = Vec::new();
        loop {
            let argument = match self.parse_type() {
                Ok(argument) => argument,
                Err(error) => {
                    let looks_like_call = self.tokens[self.cursor..]
                        .windows(2)
                        .take_while(|tokens| tokens[0].kind != TokenKind::Semicolon)
                        .any(|tokens| {
                            tokens[0].kind == TokenKind::Greater
                                && tokens[1].kind == TokenKind::LeftParen
                        });
                    self.cursor = saved;
                    if looks_like_call {
                        return Err(error);
                    }
                    return Ok(Vec::new());
                }
            };
            arguments.push(argument);
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        if self.current().kind != TokenKind::Greater {
            self.cursor = saved;
            return Ok(Vec::new());
        }
        self.advance();
        Ok(arguments)
    }

    fn parse_array_values(&mut self) -> Result<Vec<Expression>, CompileError> {
        let mut values = Vec::new();
        while self.current().kind != TokenKind::RightBracket {
            values.push(self.parse_expression(0)?);
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        Ok(values)
    }

    fn parse_arguments(&mut self) -> Result<Vec<Expression>, CompileError> {
        let mut arguments = Vec::new();
        while self.current().kind != TokenKind::RightParen {
            arguments.push(self.parse_expression(0)?);
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.advance();
        }
        Ok(arguments)
    }

    fn parse_type(&mut self) -> Result<Type, CompileError> {
        self.with_nesting(|parser| parser.parse_type_inner())
    }

    fn parse_type_inner(&mut self) -> Result<Type, CompileError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Fn => {
                self.expect_simple(TokenKind::LeftParen)?;
                let mut parameters = Vec::new();
                while self.current().kind != TokenKind::RightParen {
                    parameters.push(self.parse_type()?);
                    if self.current().kind != TokenKind::Comma {
                        break;
                    }
                    self.advance();
                }
                self.expect_simple(TokenKind::RightParen)?;
                self.expect_simple(TokenKind::Colon)?;
                Ok(Type::Function(parameters, Box::new(self.parse_type()?)))
            }
            TokenKind::Identifier(name) if name == "i8" => Ok(Type::I8),
            TokenKind::Identifier(name) if name == "i16" => Ok(Type::I16),
            TokenKind::Identifier(name) if name == "i32" => Ok(Type::I32),
            TokenKind::Identifier(name) if name == "i64" => Ok(Type::I64),
            TokenKind::Identifier(name) if name == "u8" => Ok(Type::U8),
            TokenKind::Identifier(name) if name == "u16" => Ok(Type::U16),
            TokenKind::Identifier(name) if name == "u32" => Ok(Type::U32),
            TokenKind::Identifier(name) if name == "u64" => Ok(Type::U64),
            TokenKind::Identifier(name) if name == "bool" => Ok(Type::Bool),
            TokenKind::Identifier(name) if name == "void" => Ok(Type::Void),
            TokenKind::LeftParen => {
                let first = self.parse_type()?;
                if self.current().kind != TokenKind::Comma {
                    self.expect_simple(TokenKind::RightParen)?;
                    return Ok(first);
                }
                let mut elements = vec![first];
                while self.current().kind == TokenKind::Comma {
                    self.advance();
                    if self.current().kind == TokenKind::RightParen {
                        break;
                    }
                    elements.push(self.parse_type()?);
                }
                self.expect_simple(TokenKind::RightParen)?;
                Ok(Type::Tuple(elements))
            }
            TokenKind::Identifier(name) if name == "array" => {
                self.expect_simple(TokenKind::Less)?;
                let element = self.parse_type()?;
                self.expect_simple(TokenKind::Comma)?;
                let length_token = self.advance();
                let length_span = length_token.span;
                let TokenKind::Integer(length, None) = length_token.kind else {
                    return Err(CompileError::new(
                        length_span.line,
                        length_span.column,
                        "expected array length",
                    ));
                };
                self.expect_simple(TokenKind::Greater)?;
                let length = usize::try_from(length).map_err(|_| {
                    CompileError::new(
                        length_span.line,
                        length_span.column,
                        "array length exceeds the target size",
                    )
                })?;
                Ok(Type::Array(Box::new(element), length))
            }
            TokenKind::Identifier(name) if name == "pointer" => {
                self.expect_simple(TokenKind::Less)?;
                let inner = self.parse_type()?;
                self.expect_simple(TokenKind::Greater)?;
                Ok(Type::Pointer(Box::new(inner)))
            }
            TokenKind::Identifier(name) if name == "linear" => {
                self.expect_simple(TokenKind::Less)?;
                let inner = self.parse_type()?;
                self.expect_simple(TokenKind::Greater)?;
                Ok(Type::Linear(Box::new(inner)))
            }
            TokenKind::Identifier(name) if self.type_parameters.contains(&name) => {
                Ok(Type::Parameter(name))
            }
            TokenKind::Identifier(name) => Err(CompileError::new(
                token.span.line,
                token.span.column,
                format!("unknown type `{name}`"),
            )),
            _ => Err(CompileError::new(
                token.span.line,
                token.span.column,
                "expected a type",
            )),
        }
    }

    fn expect_identifier(&mut self) -> Result<String, CompileError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            _ => Err(CompileError::new(
                token.span.line,
                token.span.column,
                "expected an identifier",
            )),
        }
    }

    fn expression(&mut self, span: crate::lexer::Span, kind: ExpressionKind) -> Expression {
        Expression { kind, span }
    }

    fn with_nesting<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, CompileError>,
    ) -> Result<T, CompileError> {
        if self.nesting_depth >= MAX_NESTING_DEPTH {
            return self.error("maximum parser nesting depth exceeded");
        }
        self.nesting_depth += 1;
        let result = parse(self);
        self.nesting_depth -= 1;
        result
    }

    fn expect_simple(&mut self, expected: TokenKind) -> Result<Token, CompileError> {
        if self.current().kind == expected {
            Ok(self.advance())
        } else {
            self.error(&format!("expected `{}`", token_name(&expected)))
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn next(&self) -> &Token {
        &self.tokens[self.cursor + 1]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        token
    }

    fn error<T>(&self, message: &str) -> Result<T, CompileError> {
        let span = self.current().span;
        Err(CompileError::new(span.line, span.column, message))
    }
}

fn binary_operator(kind: &TokenKind) -> Option<(BinaryOperator, u8)> {
    match kind {
        TokenKind::EqualEqual => Some((BinaryOperator::Equal, 1)),
        TokenKind::BangEqual => Some((BinaryOperator::NotEqual, 1)),
        TokenKind::Less => Some((BinaryOperator::Less, 2)),
        TokenKind::LessEqual => Some((BinaryOperator::LessEqual, 2)),
        TokenKind::Greater => Some((BinaryOperator::Greater, 2)),
        TokenKind::GreaterEqual => Some((BinaryOperator::GreaterEqual, 2)),
        TokenKind::Plus => Some((BinaryOperator::Add, 3)),
        TokenKind::Minus => Some((BinaryOperator::Subtract, 3)),
        TokenKind::Star => Some((BinaryOperator::Multiply, 4)),
        TokenKind::Slash => Some((BinaryOperator::Divide, 4)),
        _ => None,
    }
}

fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Fn => "fn",
        TokenKind::Extern => "extern",
        TokenKind::Else => "else",
        TokenKind::For => "for",
        TokenKind::LeftParen => "(",
        TokenKind::RightParen => ")",
        TokenKind::LeftBrace => "{",
        TokenKind::RightBrace => "}",
        TokenKind::LeftBracket => "[",
        TokenKind::RightBracket => "]",
        TokenKind::Colon => ":",
        TokenKind::Comma => ",",
        TokenKind::Semicolon => ";",
        TokenKind::Equal => "=",
        _ => "token",
    }
}

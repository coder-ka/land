use std::collections::HashMap;
use std::fmt;

use crate::lexer::Span;

#[derive(Debug)]
pub struct Program {
    pub imports: Vec<Import>,
    pub functions: Vec<Function>,
}

#[derive(Debug)]
pub struct Import {
    pub names: Vec<ImportName>,
    pub module: String,
    pub span: Span,
}

#[derive(Debug)]
pub struct ImportName {
    pub imported: String,
    pub local: String,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub exported: bool,
    pub visible_functions: Option<HashMap<String, String>>,
    pub type_parameters: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub body: Option<Vec<Statement>>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Parameter {
    pub name: String,
    pub parameter_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Bool,
    Void,
    Tuple(Vec<Type>),
    Array(Box<Type>, usize),
    Pointer(Box<Type>),
    Linear(Box<Type>),
    Parameter(String),
    Function(Vec<Type>, Box<Type>),
}

impl Type {
    pub fn has_runtime_value(&self) -> bool {
        match self {
            Self::I8
            | Self::I16
            | Self::I32
            | Self::I64
            | Self::U8
            | Self::U16
            | Self::U32
            | Self::U64
            | Self::Bool
            | Self::Parameter(_)
            | Self::Function(_, _) => true,
            Self::Void => false,
            Self::Tuple(elements) => elements.iter().all(Self::has_runtime_value),
            Self::Array(element, _) | Self::Pointer(element) | Self::Linear(element) => {
                element.has_runtime_value()
            }
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::I8
                | Self::I16
                | Self::I32
                | Self::I64
                | Self::U8
                | Self::U16
                | Self::U32
                | Self::U64
        )
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    pub fn integer_max(&self) -> Option<i128> {
        match self {
            Self::I8 => Some(i8::MAX.into()),
            Self::I16 => Some(i16::MAX.into()),
            Self::I32 => Some(i32::MAX.into()),
            Self::I64 => Some(i64::MAX.into()),
            Self::U8 => Some(u8::MAX.into()),
            Self::U16 => Some(u16::MAX.into()),
            Self::U32 => Some(u32::MAX.into()),
            Self::U64 => Some(u64::MAX.into()),
            _ => None,
        }
    }

    pub fn is_linear(&self) -> bool {
        match self {
            Self::Linear(_) | Self::Parameter(_) => true,
            Self::Array(element, _) => element.is_linear(),
            Self::Tuple(elements) => elements.iter().any(Self::is_linear),
            _ => false,
        }
    }

    pub fn from_integer_suffix(suffix: Option<&str>) -> Option<Self> {
        match suffix.unwrap_or("i32") {
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            _ => None,
        }
    }

    pub fn supports_equality(&self) -> bool {
        match self {
            Self::Bool | Self::Pointer(_) => true,
            Self::Linear(inner) => inner.supports_equality(),
            value_type => value_type.is_integer(),
        }
    }

    pub fn substitute(&self, substitutions: &HashMap<String, Self>) -> Self {
        match self {
            Self::Parameter(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Self::Tuple(elements) => Self::Tuple(
                elements
                    .iter()
                    .map(|element| element.substitute(substitutions))
                    .collect(),
            ),
            Self::Array(element, length) => {
                Self::Array(Box::new(element.substitute(substitutions)), *length)
            }
            Self::Pointer(inner) => Self::Pointer(Box::new(inner.substitute(substitutions))),
            Self::Linear(inner) => Self::Linear(Box::new(inner.substitute(substitutions))),
            Self::Function(parameters, result) => Self::Function(
                parameters
                    .iter()
                    .map(|parameter| parameter.substitute(substitutions))
                    .collect(),
                Box::new(result.substitute(substitutions)),
            ),
            _ => self.clone(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I8 => write!(formatter, "i8"),
            Self::I16 => write!(formatter, "i16"),
            Self::I32 => write!(formatter, "i32"),
            Self::I64 => write!(formatter, "i64"),
            Self::U8 => write!(formatter, "u8"),
            Self::U16 => write!(formatter, "u16"),
            Self::U32 => write!(formatter, "u32"),
            Self::U64 => write!(formatter, "u64"),
            Self::Bool => write!(formatter, "bool"),
            Self::Void => write!(formatter, "void"),
            Self::Tuple(elements) => write!(
                formatter,
                "({})",
                elements
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Array(element, length) => write!(formatter, "array<{element}, {length}>"),
            Self::Pointer(inner) => write!(formatter, "pointer<{inner}>"),
            Self::Linear(inner) => write!(formatter, "linear<{inner}>"),
            Self::Parameter(name) => write!(formatter, "{name}"),
            Self::Function(parameters, result) => write!(
                formatter,
                "fn({}): {result}",
                parameters
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[derive(Debug)]
pub enum Statement {
    Let {
        pattern: Pattern,
        annotation: Option<Type>,
        value: Expression,
        span: Span,
    },
    Assign {
        name: String,
        value: Expression,
        span: Span,
    },
    Expression(Expression),
    Return {
        value: Option<Expression>,
        span: Span,
    },
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
        span: Span,
    },
    For {
        initializer: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Box<Statement>>,
        body: Vec<Statement>,
        span: Span,
    },
}

#[derive(Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum PatternKind {
    Name(String),
    Tuple(Vec<Pattern>),
}

#[derive(Debug)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExpressionKind {
    Integer {
        value: i128,
        suffix: Option<String>,
    },
    Boolean(bool),
    Name(String),
    Call {
        callee: Box<Expression>,
        type_arguments: Vec<Type>,
        arguments: Vec<Expression>,
    },
    Tuple(Vec<Expression>),
    Array(Vec<Expression>),
    Index {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOperator {
    Negate,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

use std::collections::{HashMap, HashSet};

use crate::CompileError;
use crate::ast::{self, BinaryOperator, Type, UnaryOperator};
use crate::lexer::Span;

pub type FunctionId = usize;
pub type LocalId = usize;

pub struct Program {
    pub functions: Vec<Function>,
}

pub struct Function {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
    pub body: Option<Vec<Statement>>,
    pub locals: Vec<Local>,
}

pub struct Local {
    pub name: String,
    pub value_type: Type,
}

pub struct Parameter {
    pub name: String,
    pub local: LocalId,
    pub parameter_type: Type,
}

pub enum Statement {
    Let {
        pattern: Pattern,
        value: Expression,
    },
    Assign {
        local: LocalId,
        value: Expression,
    },
    Expression(Expression),
    Return(Option<Expression>),
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    For {
        initializer: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Box<Statement>>,
        body: Vec<Statement>,
    },
}

pub struct Pattern {
    pub kind: PatternKind,
    pub value_type: Type,
}

pub enum PatternKind {
    Bind(LocalId),
    Tuple(Vec<Pattern>),
}

pub struct Expression {
    pub kind: ExpressionKind,
    pub result_type: Type,
    pub span: Span,
}

pub enum ExpressionKind {
    Integer(i128),
    Boolean(bool),
    Local(LocalId),
    Function(FunctionId),
    DirectCall {
        function: FunctionId,
        type_arguments: Vec<Type>,
        arguments: Vec<Expression>,
    },
    IndirectCall {
        callee: Box<Expression>,
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

pub fn analyze(source: &ast::Program) -> Result<Program, CompileError> {
    let mut function_names = HashMap::new();
    for (id, function) in source.functions.iter().enumerate() {
        if function_names.insert(function.name.as_str(), id).is_some() {
            return Err(CompileError::new(
                function.span.line,
                function.span.column,
                format!("duplicate function `{}`", function.name),
            ));
        }
    }

    // Validate all declarations first so diagnostics are independent of body order.
    for function in &source.functions {
        let mut parameters = HashSet::new();
        for parameter in &function.parameters {
            if !parameters.insert(parameter.name.as_str()) {
                return Err(CompileError::new(
                    parameter.span.line,
                    parameter.span.column,
                    format!("duplicate parameter `{}`", parameter.name),
                ));
            }
            validate_value_type(&parameter.parameter_type, parameter.span, "parameter")?;
        }
        validate_return_type(&function.return_type, function.span)?;
    }

    let mut functions = Vec::with_capacity(source.functions.len());
    for function in &source.functions {
        let visible_functions = if let Some(aliases) = &function.visible_functions {
            aliases
                .iter()
                .map(|(name, target)| {
                    function_names
                        .get(target.as_str())
                        .copied()
                        .map(|id| (name.as_str(), id))
                        .ok_or_else(|| {
                            CompileError::new(
                                function.span.line,
                                function.span.column,
                                format!("unknown imported function `{name}`"),
                            )
                        })
                })
                .collect::<Result<HashMap<_, _>, _>>()?
        } else {
            function_names.clone()
        };
        functions.push(Checker::new(source, &visible_functions).lower_function(function)?);
    }
    Ok(Program { functions })
}

struct Binding {
    name: String,
    value_type: Type,
    linear: bool,
    span: Span,
}

struct Environment {
    scopes: Vec<HashMap<String, LocalId>>,
    bindings: Vec<Binding>,
    availability: Vec<bool>,
}

impl Environment {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            bindings: Vec::new(),
            availability: Vec::new(),
        }
    }

    fn resolve(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn bind(&mut self, name: String, value_type: Type, span: Span) -> LocalId {
        let local = self.bindings.len();
        self.bindings.push(Binding {
            name: name.clone(),
            linear: value_type.is_linear(),
            value_type,
            span,
        });
        self.availability.push(true);
        self.scopes.last_mut().unwrap().insert(name, local);
        local
    }

    fn snapshot(&self) -> Vec<bool> {
        self.availability.clone()
    }

    fn restore(&mut self, state: &[bool]) {
        self.availability[..state.len()].copy_from_slice(state);
    }

    fn outer_state(&self, length: usize) -> Vec<bool> {
        self.availability[..length].to_vec()
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self, check_available: bool) -> Result<(), CompileError> {
        let scope = self.scopes.pop().unwrap();
        if check_available {
            for local in scope.values() {
                let binding = &self.bindings[*local];
                if binding.linear && self.availability[*local] {
                    return Err(CompileError::new(
                        binding.span.line,
                        binding.span.column,
                        format!(
                            "linear value `{}` leaves scope without being moved",
                            binding.name
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn ensure_all_moved(&self) -> Result<(), CompileError> {
        for scope in self.scopes.iter().rev() {
            for local in scope.values() {
                let binding = &self.bindings[*local];
                if binding.linear && self.availability[*local] {
                    return Err(CompileError::new(
                        binding.span.line,
                        binding.span.column,
                        format!("linear value `{}` is not moved before return", binding.name),
                    ));
                }
            }
        }
        Ok(())
    }
}

struct Checker<'a> {
    source: &'a ast::Program,
    functions: &'a HashMap<&'a str, FunctionId>,
    environment: Environment,
    return_type: Type,
}

impl<'a> Checker<'a> {
    fn new(source: &'a ast::Program, functions: &'a HashMap<&'a str, FunctionId>) -> Self {
        Self {
            source,
            functions,
            environment: Environment::new(),
            return_type: Type::Void,
        }
    }

    fn lower_function(mut self, function: &ast::Function) -> Result<Function, CompileError> {
        if function.body.is_none() {
            self.check_extern(function)?;
        }
        self.return_type = function.return_type.clone();
        let parameters = function
            .parameters
            .iter()
            .map(|parameter| Parameter {
                name: parameter.name.clone(),
                local: self.environment.bind(
                    parameter.name.clone(),
                    parameter.parameter_type.clone(),
                    parameter.span,
                ),
                parameter_type: parameter.parameter_type.clone(),
            })
            .collect();
        let body = if let Some(body) = &function.body {
            let (body, terminated) = self.lower_statements(body)?;
            if !terminated {
                return Err(CompileError::new(
                    function.span.line,
                    function.span.column,
                    format!("function `{}` must return on every path", function.name),
                ));
            }
            Some(body)
        } else {
            None
        };
        let locals = self
            .environment
            .bindings
            .iter()
            .map(|binding| Local {
                name: binding.name.clone(),
                value_type: binding.value_type.clone(),
            })
            .collect();
        Ok(Function {
            name: function.name.clone(),
            type_parameters: function.type_parameters.clone(),
            parameters,
            return_type: function.return_type.clone(),
            body,
            locals,
        })
    }

    fn check_extern(&self, function: &ast::Function) -> Result<(), CompileError> {
        if !function.type_parameters.is_empty() {
            return Err(CompileError::new(
                function.span.line,
                function.span.column,
                "extern functions cannot have type parameters",
            ));
        }
        for parameter in &function.parameters {
            if !is_supported_c_abi_type(&parameter.parameter_type, false) {
                return Err(CompileError::new(
                    parameter.span.line,
                    parameter.span.column,
                    format!(
                        "{} is not yet supported at the C ABI boundary",
                        parameter.parameter_type
                    ),
                ));
            }
        }
        if !is_supported_c_abi_type(&function.return_type, true) {
            return Err(CompileError::new(
                function.span.line,
                function.span.column,
                format!(
                    "{} is not yet supported at the C ABI boundary",
                    function.return_type
                ),
            ));
        }
        Ok(())
    }

    fn lower_statements(
        &mut self,
        statements: &[ast::Statement],
    ) -> Result<(Vec<Statement>, bool), CompileError> {
        let mut result = Vec::with_capacity(statements.len());
        let mut terminated = false;
        for statement in statements {
            if terminated {
                let span = statement_span(statement);
                return Err(CompileError::new(
                    span.line,
                    span.column,
                    "statement appears after return",
                ));
            }
            let (statement, statement_terminated) = self.lower_statement(statement)?;
            result.push(statement);
            terminated = statement_terminated;
        }
        Ok((result, terminated))
    }

    fn lower_statement(
        &mut self,
        statement: &ast::Statement,
    ) -> Result<(Statement, bool), CompileError> {
        match statement {
            ast::Statement::Let {
                pattern,
                annotation,
                value,
                span,
            } => {
                let value = self.lower_expression(value)?;
                if !value.result_type.has_runtime_value() {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        "void value cannot be bound to a name",
                    ));
                }
                let binding_type = annotation.as_ref().unwrap_or(&value.result_type);
                if binding_type != &value.result_type {
                    return self.type_error(binding_type, &value.result_type, *span);
                }
                let mut names = HashSet::new();
                ensure_unique_pattern_names(pattern, &mut names)?;
                let pattern = self.lower_pattern(pattern, binding_type)?;
                Ok((Statement::Let { pattern, value }, false))
            }
            ast::Statement::Assign { name, value, span } => {
                let local = self.environment.resolve(name).ok_or_else(|| {
                    CompileError::new(span.line, span.column, format!("unknown name `{name}`"))
                })?;
                let target_type = self.environment.bindings[local].value_type.clone();
                if self.environment.bindings[local].linear && self.environment.availability[local] {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        format!("linear value `{name}` cannot be overwritten before being moved"),
                    ));
                }
                let value = self.lower_expression(value)?;
                if target_type != value.result_type {
                    return self.type_error(&target_type, &value.result_type, *span);
                }
                self.environment.availability[local] = true;
                Ok((Statement::Assign { local, value }, false))
            }
            ast::Statement::Expression(expression) => {
                let expression = self.lower_expression(expression)?;
                if expression.result_type.is_linear() {
                    return Err(CompileError::new(
                        expression.span.line,
                        expression.span.column,
                        "linear return value cannot be discarded",
                    ));
                }
                Ok((Statement::Expression(expression), false))
            }
            ast::Statement::Return { value, span } => {
                let value = value
                    .as_ref()
                    .map(|value| self.lower_expression(value))
                    .transpose()?;
                let actual = value
                    .as_ref()
                    .map_or(&Type::Void, |value| &value.result_type);
                if actual != &self.return_type {
                    return self.type_error(&self.return_type, actual, *span);
                }
                self.environment.ensure_all_moved()?;
                Ok((Statement::Return(value), true))
            }
            ast::Statement::If {
                condition,
                then_body,
                else_body,
                span,
            } => self.lower_if(condition, then_body, else_body.as_deref(), *span),
            ast::Statement::For {
                initializer,
                condition,
                update,
                body,
                span,
            } => self.lower_for(
                initializer.as_deref(),
                condition.as_ref(),
                update.as_deref(),
                body,
                *span,
            ),
        }
    }

    fn lower_if(
        &mut self,
        condition: &ast::Expression,
        then_body: &[ast::Statement],
        else_body: Option<&[ast::Statement]>,
        span: Span,
    ) -> Result<(Statement, bool), CompileError> {
        let condition = self.lower_expression(condition)?;
        if condition.result_type != Type::Bool {
            return self.type_error(&Type::Bool, &condition.result_type, condition.span);
        }
        let base = self.environment.snapshot();
        let (then_body, then_state, then_terminated) =
            self.lower_scoped_branch(then_body, &base)?;
        let (else_body, else_state, else_terminated) = if let Some(else_body) = else_body {
            let (body, state, terminated) = self.lower_scoped_branch(else_body, &base)?;
            (Some(body), state, terminated)
        } else {
            self.environment.restore(&base);
            (None, base.clone(), false)
        };

        let terminated = match (then_terminated, else_terminated) {
            (true, true) => true,
            (true, false) => {
                self.environment.restore(&else_state);
                false
            }
            (false, true) => {
                self.environment.restore(&then_state);
                false
            }
            (false, false) => {
                if then_state != else_state {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        "linear value has different states after if branches",
                    ));
                }
                self.environment.restore(&then_state);
                false
            }
        };
        Ok((
            Statement::If {
                condition,
                then_body,
                else_body,
            },
            terminated,
        ))
    }

    fn lower_scoped_branch(
        &mut self,
        statements: &[ast::Statement],
        base: &[bool],
    ) -> Result<(Vec<Statement>, Vec<bool>, bool), CompileError> {
        self.environment.restore(base);
        self.environment.push_scope();
        let (statements, terminated) = self.lower_statements(statements)?;
        self.environment.pop_scope(!terminated)?;
        let state = self.environment.outer_state(base.len());
        Ok((statements, state, terminated))
    }

    fn lower_for(
        &mut self,
        initializer: Option<&ast::Statement>,
        condition: Option<&ast::Expression>,
        update: Option<&ast::Statement>,
        body: &[ast::Statement],
        span: Span,
    ) -> Result<(Statement, bool), CompileError> {
        self.environment.push_scope();
        let initializer = match initializer {
            Some(initializer) => Some(Box::new(self.lower_statement(initializer)?.0)),
            None => None,
        };
        let loop_entry = self.environment.snapshot();
        let length = loop_entry.len();

        let condition = match condition {
            Some(condition) => {
                let condition = self.lower_expression(condition)?;
                if condition.result_type != Type::Bool {
                    return self.type_error(&Type::Bool, &condition.result_type, condition.span);
                }
                if self.environment.outer_state(length) != loop_entry {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        "for condition cannot move a linear value",
                    ));
                }
                Some(condition)
            }
            None => None,
        };

        let (body, after_body, body_terminated) = self.lower_scoped_branch(body, &loop_entry)?;
        if !body_terminated && loop_entry != after_body {
            return Err(CompileError::new(
                span.line,
                span.column,
                "linear value must have the same state after each loop iteration",
            ));
        }

        let update = match update {
            Some(update) => {
                self.environment.restore(&loop_entry);
                let update = Box::new(self.lower_statement(update)?.0);
                if self.environment.outer_state(length) != loop_entry {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        "linear value must have the same state after each loop iteration",
                    ));
                }
                Some(update)
            }
            None => {
                self.environment.restore(&loop_entry);
                None
            }
        };

        let terminated = condition
            .as_ref()
            .is_none_or(|condition| matches!(&condition.kind, ExpressionKind::Boolean(true)));
        self.environment.restore(&loop_entry);
        self.environment.pop_scope(!terminated)?;
        Ok((
            Statement::For {
                initializer,
                condition,
                update,
                body,
            },
            terminated,
        ))
    }

    fn lower_expression(
        &mut self,
        expression: &ast::Expression,
    ) -> Result<Expression, CompileError> {
        let (kind, result_type) = match &expression.kind {
            ast::ExpressionKind::Integer { value, suffix } => {
                let value_type = integer_type(suffix.as_deref(), expression.span)?;
                if value_type
                    .integer_max()
                    .is_some_and(|maximum| *value > maximum)
                {
                    return Err(CompileError::new(
                        expression.span.line,
                        expression.span.column,
                        format!("integer literal does not fit in {value_type}"),
                    ));
                }
                (ExpressionKind::Integer(*value), value_type)
            }
            ast::ExpressionKind::Boolean(value) => (ExpressionKind::Boolean(*value), Type::Bool),
            ast::ExpressionKind::Name(name) => return self.lower_name(name, expression.span),
            ast::ExpressionKind::Call {
                callee,
                type_arguments,
                arguments,
            } => return self.lower_call(callee, type_arguments, arguments, expression.span),
            ast::ExpressionKind::Tuple(values) => {
                let values = values
                    .iter()
                    .map(|value| self.lower_expression(value))
                    .collect::<Result<Vec<_>, _>>()?;
                let value_type = Type::Tuple(
                    values
                        .iter()
                        .map(|value| value.result_type.clone())
                        .collect(),
                );
                (ExpressionKind::Tuple(values), value_type)
            }
            ast::ExpressionKind::Array(values) => {
                let Some(first) = values.first() else {
                    return Err(CompileError::new(
                        expression.span.line,
                        expression.span.column,
                        "cannot infer the type of an empty array",
                    ));
                };
                let mut lowered = vec![self.lower_expression(first)?];
                let element_type = lowered[0].result_type.clone();
                for value in &values[1..] {
                    let value = self.lower_expression(value)?;
                    if value.result_type != element_type {
                        return self.type_error(&element_type, &value.result_type, value.span);
                    }
                    lowered.push(value);
                }
                let value_type = Type::Array(Box::new(element_type), lowered.len());
                (ExpressionKind::Array(lowered), value_type)
            }
            ast::ExpressionKind::Index { base, index } => {
                let base = self.lower_expression(base)?;
                if base.result_type.is_linear() {
                    return Err(CompileError::new(
                        expression.span.line,
                        expression.span.column,
                        "linear arrays cannot be indexed",
                    ));
                }
                let index = self.lower_expression(index)?;
                if index.result_type != Type::I32 {
                    return self.type_error(&Type::I32, &index.result_type, index.span);
                }
                let Type::Array(element, length) = &base.result_type else {
                    return Err(CompileError::new(
                        expression.span.line,
                        expression.span.column,
                        "indexing requires an array value",
                    ));
                };
                if let ExpressionKind::Integer(value) = index.kind
                    && usize::try_from(value).is_ok_and(|index| index >= *length)
                {
                    return Err(CompileError::new(
                        index.span.line,
                        index.span.column,
                        format!("array index {value} is out of bounds for length {length}"),
                    ));
                }
                let result_type = (**element).clone();
                (
                    ExpressionKind::Index {
                        base: Box::new(base),
                        index: Box::new(index),
                    },
                    result_type,
                )
            }
            ast::ExpressionKind::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } => {
                let operand = if let ast::ExpressionKind::Integer { value, suffix } = &operand.kind
                {
                    let value_type = integer_type(suffix.as_deref(), operand.span)?;
                    if !value_type.is_signed_integer() {
                        return Err(CompileError::new(
                            expression.span.line,
                            expression.span.column,
                            "negation requires a signed integer",
                        ));
                    }
                    if value_type
                        .integer_max()
                        .is_some_and(|maximum| *value > maximum + 1)
                    {
                        return Err(CompileError::new(
                            operand.span.line,
                            operand.span.column,
                            format!("integer literal does not fit in {value_type}"),
                        ));
                    }
                    Expression {
                        kind: ExpressionKind::Integer(*value),
                        result_type: value_type,
                        span: operand.span,
                    }
                } else {
                    let operand = self.lower_expression(operand)?;
                    if !operand.result_type.is_signed_integer() {
                        return Err(CompileError::new(
                            expression.span.line,
                            expression.span.column,
                            "negation requires a signed integer",
                        ));
                    }
                    operand
                };
                let result_type = operand.result_type.clone();
                (
                    ExpressionKind::Unary {
                        operator: UnaryOperator::Negate,
                        operand: Box::new(operand),
                    },
                    result_type,
                )
            }
            ast::ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.lower_expression(left)?;
                let right = self.lower_expression(right)?;
                if left.result_type != right.result_type {
                    return self.type_error(&left.result_type, &right.result_type, expression.span);
                }
                if matches!(operator, BinaryOperator::Divide)
                    && matches!(right.kind, ExpressionKind::Integer(0))
                {
                    return Err(CompileError::new(
                        right.span.line,
                        right.span.column,
                        "division by zero",
                    ));
                }
                let result_type = match operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide => {
                        if !left.result_type.is_integer() {
                            return Err(CompileError::new(
                                expression.span.line,
                                expression.span.column,
                                "arithmetic requires integer operands",
                            ));
                        }
                        left.result_type.clone()
                    }
                    BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual => {
                        if !left.result_type.is_integer() {
                            return Err(CompileError::new(
                                expression.span.line,
                                expression.span.column,
                                "ordering requires integer operands",
                            ));
                        }
                        Type::Bool
                    }
                    BinaryOperator::Equal | BinaryOperator::NotEqual => {
                        if left.result_type.is_linear() {
                            return Err(CompileError::new(
                                expression.span.line,
                                expression.span.column,
                                "linear values cannot be compared",
                            ));
                        }
                        if !left.result_type.supports_equality() {
                            return Err(CompileError::new(
                                expression.span.line,
                                expression.span.column,
                                "equality is only defined for scalar values",
                            ));
                        }
                        Type::Bool
                    }
                };
                (
                    ExpressionKind::Binary {
                        operator: *operator,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    result_type,
                )
            }
        };
        Ok(Expression {
            kind,
            result_type,
            span: expression.span,
        })
    }

    fn lower_name(&mut self, name: &str, span: Span) -> Result<Expression, CompileError> {
        if let Some(local) = self.environment.resolve(name) {
            let binding = &self.environment.bindings[local];
            if binding.linear {
                if !self.environment.availability[local] {
                    return Err(CompileError::new(
                        span.line,
                        span.column,
                        format!("linear value `{name}` was already moved"),
                    ));
                }
                self.environment.availability[local] = false;
            }
            return Ok(Expression {
                kind: ExpressionKind::Local(local),
                result_type: binding.value_type.clone(),
                span,
            });
        }
        let function = *self.functions.get(name).ok_or_else(|| {
            CompileError::new(span.line, span.column, format!("unknown name `{name}`"))
        })?;
        let declaration = &self.source.functions[function];
        if !declaration.type_parameters.is_empty() {
            return Err(CompileError::new(
                span.line,
                span.column,
                "generic function cannot be used as a value",
            ));
        }
        Ok(Expression {
            kind: ExpressionKind::Function(function),
            result_type: function_type(declaration, &HashMap::new()),
            span,
        })
    }

    fn lower_call(
        &mut self,
        callee: &ast::Expression,
        type_arguments: &[Type],
        arguments: &[ast::Expression],
        span: Span,
    ) -> Result<Expression, CompileError> {
        if let ast::ExpressionKind::Name(name) = &callee.kind
            && self.environment.resolve(name).is_none()
        {
            let function = *self.functions.get(name.as_str()).ok_or_else(|| {
                CompileError::new(span.line, span.column, format!("unknown function `{name}`"))
            })?;
            let declaration = &self.source.functions[function];
            if type_arguments.len() != declaration.type_parameters.len() {
                return Err(CompileError::new(
                    span.line,
                    span.column,
                    format!(
                        "function `{name}` expects {} type arguments, got {}",
                        declaration.type_parameters.len(),
                        type_arguments.len()
                    ),
                ));
            }
            let substitutions = declaration
                .type_parameters
                .iter()
                .cloned()
                .zip(type_arguments.iter().cloned())
                .collect::<HashMap<_, _>>();
            let parameters = declaration
                .parameters
                .iter()
                .map(|parameter| parameter.parameter_type.substitute(&substitutions))
                .collect::<Vec<_>>();
            let arguments = self.lower_arguments(Some(name), arguments, &parameters, span)?;
            let result_type = declaration.return_type.substitute(&substitutions);
            return Ok(Expression {
                kind: ExpressionKind::DirectCall {
                    function,
                    type_arguments: type_arguments.to_vec(),
                    arguments,
                },
                result_type,
                span,
            });
        }
        if !type_arguments.is_empty() {
            return Err(CompileError::new(
                span.line,
                span.column,
                "function values do not accept type arguments",
            ));
        }
        let callee = self.lower_expression(callee)?;
        let Type::Function(parameters, result_type) = &callee.result_type else {
            return Err(CompileError::new(
                callee.span.line,
                callee.span.column,
                format!("value of type {} is not callable", callee.result_type),
            ));
        };
        let arguments = self.lower_arguments(None, arguments, parameters, span)?;
        let result_type = (**result_type).clone();
        Ok(Expression {
            kind: ExpressionKind::IndirectCall {
                callee: Box::new(callee),
                arguments,
            },
            result_type,
            span,
        })
    }

    fn lower_arguments(
        &mut self,
        function: Option<&str>,
        arguments: &[ast::Expression],
        parameters: &[Type],
        span: Span,
    ) -> Result<Vec<Expression>, CompileError> {
        if arguments.len() != parameters.len() {
            return Err(CompileError::new(
                span.line,
                span.column,
                if let Some(function) = function {
                    format!(
                        "function `{function}` expects {} arguments, got {}",
                        parameters.len(),
                        arguments.len()
                    )
                } else {
                    format!(
                        "function call expects {} arguments, got {}",
                        parameters.len(),
                        arguments.len()
                    )
                },
            ));
        }
        let mut lowered = Vec::with_capacity(arguments.len());
        for (argument, expected) in arguments.iter().zip(parameters) {
            let argument = self.lower_expression(argument)?;
            if &argument.result_type != expected {
                return self.type_error(expected, &argument.result_type, argument.span);
            }
            lowered.push(argument);
        }
        Ok(lowered)
    }

    fn lower_pattern(
        &mut self,
        pattern: &ast::Pattern,
        value_type: &Type,
    ) -> Result<Pattern, CompileError> {
        let kind = match &pattern.kind {
            ast::PatternKind::Name(name) => {
                if let Some(local) = self.environment.scopes.last().unwrap().get(name)
                    && self.environment.bindings[*local].linear
                    && self.environment.availability[*local]
                {
                    return Err(CompileError::new(
                        pattern.span.line,
                        pattern.span.column,
                        format!("linear value `{name}` cannot be shadowed before being moved"),
                    ));
                }
                PatternKind::Bind(self.environment.bind(
                    name.clone(),
                    value_type.clone(),
                    pattern.span,
                ))
            }
            ast::PatternKind::Tuple(patterns) => {
                let Type::Tuple(elements) = value_type else {
                    return Err(CompileError::new(
                        pattern.span.line,
                        pattern.span.column,
                        format!("tuple pattern requires a tuple, got {value_type}"),
                    ));
                };
                if patterns.len() != elements.len() {
                    return Err(CompileError::new(
                        pattern.span.line,
                        pattern.span.column,
                        format!(
                            "tuple pattern expects {} elements, got {}",
                            elements.len(),
                            patterns.len()
                        ),
                    ));
                }
                PatternKind::Tuple(
                    patterns
                        .iter()
                        .zip(elements)
                        .map(|(pattern, element)| self.lower_pattern(pattern, element))
                        .collect::<Result<_, _>>()?,
                )
            }
        };
        Ok(Pattern {
            kind,
            value_type: value_type.clone(),
        })
    }

    fn type_error<T>(&self, expected: &Type, actual: &Type, span: Span) -> Result<T, CompileError> {
        Err(CompileError::new(
            span.line,
            span.column,
            format!("expected {expected}, got {actual}"),
        ))
    }
}

fn integer_type(suffix: Option<&str>, span: Span) -> Result<Type, CompileError> {
    Type::from_integer_suffix(suffix).ok_or_else(|| {
        CompileError::new(
            span.line,
            span.column,
            format!("unknown integer suffix `{}`", suffix.unwrap_or_default()),
        )
    })
}

fn function_type(function: &ast::Function, substitutions: &HashMap<String, Type>) -> Type {
    Type::Function(
        function
            .parameters
            .iter()
            .map(|parameter| parameter.parameter_type.substitute(substitutions))
            .collect(),
        Box::new(function.return_type.substitute(substitutions)),
    )
}

fn ensure_unique_pattern_names(
    pattern: &ast::Pattern,
    names: &mut HashSet<String>,
) -> Result<(), CompileError> {
    match &pattern.kind {
        ast::PatternKind::Name(name) => {
            if !names.insert(name.clone()) {
                return Err(CompileError::new(
                    pattern.span.line,
                    pattern.span.column,
                    format!("duplicate name `{name}` in binding pattern"),
                ));
            }
        }
        ast::PatternKind::Tuple(patterns) => {
            for pattern in patterns {
                ensure_unique_pattern_names(pattern, names)?;
            }
        }
    }
    Ok(())
}

fn is_supported_c_abi_type(value_type: &Type, allow_void: bool) -> bool {
    match value_type {
        Type::I32 | Type::I64 | Type::U32 | Type::U64 | Type::Pointer(_) => true,
        Type::Void => allow_void,
        Type::Linear(inner) => is_supported_c_abi_type(inner, false),
        Type::Function(parameters, result) => {
            parameters
                .iter()
                .all(|parameter| is_supported_c_abi_type(parameter, false))
                && is_supported_c_abi_type(result, true)
        }
        _ => false,
    }
}

fn validate_value_type(value_type: &Type, span: Span, position: &str) -> Result<(), CompileError> {
    match value_type {
        Type::Void => Err(CompileError::new(
            span.line,
            span.column,
            format!("void cannot be used in a {position} type"),
        )),
        Type::Tuple(elements) => {
            for element in elements {
                validate_value_type(element, span, "tuple element")?;
            }
            Ok(())
        }
        Type::Array(element, _) => validate_value_type(element, span, "array element"),
        Type::Pointer(inner) => validate_value_type(inner, span, "pointer target"),
        Type::Linear(inner) => validate_value_type(inner, span, "linear value"),
        Type::Function(parameters, result) => {
            for parameter in parameters {
                validate_value_type(parameter, span, "function parameter")?;
            }
            validate_return_type(result, span)
        }
        _ => Ok(()),
    }
}

fn validate_return_type(value_type: &Type, span: Span) -> Result<(), CompileError> {
    if *value_type == Type::Void {
        Ok(())
    } else {
        validate_value_type(value_type, span, "return")
    }
}

fn statement_span(statement: &ast::Statement) -> Span {
    match statement {
        ast::Statement::Let { span, .. }
        | ast::Statement::Assign { span, .. }
        | ast::Statement::If { span, .. }
        | ast::Statement::For { span, .. }
        | ast::Statement::Return { span, .. } => *span,
        ast::Statement::Expression(expression) => expression.span,
    }
}

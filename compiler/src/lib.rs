mod ast;
mod codegen;
mod lexer;
mod module;
mod parser;
mod semantic;

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl CompileError {
    fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for CompileError {}

pub fn compile(source: &str) -> Result<String, CompileError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    if let Some(import) = program.imports.first() {
        return Err(CompileError::new(
            import.span.line,
            import.span.column,
            "imports require file-based compilation",
        ));
    }
    if let Some(function) = program
        .functions
        .iter()
        .find(|function| function.body.is_none())
    {
        return Err(CompileError::new(
            function.span.line,
            function.span.column,
            "extern declarations are only allowed in .landh files",
        ));
    }
    let resolved = semantic::analyze(&program)?;
    codegen::generate(&resolved)
}

pub fn compile_file(path: impl AsRef<Path>) -> Result<String, String> {
    let program = module::load(path.as_ref())?.program;
    let resolved = semantic::analyze(&program).map_err(|error| error.to_string())?;
    codegen::generate(&resolved).map_err(|error| error.to_string())
}

pub fn compile_executable(path: impl AsRef<Path>) -> Result<String, String> {
    Ok(compile_executable_with_headers(path)?.0)
}

pub fn compile_executable_with_headers(
    path: impl AsRef<Path>,
) -> Result<(String, Vec<std::path::PathBuf>), String> {
    let loaded = module::load(path.as_ref())?;
    let program = loaded.program;
    let main = program
        .functions
        .iter()
        .find(|function| function.name == "main")
        .ok_or_else(|| "executable requires `fn main(): i32`".to_owned())?;
    if !main.type_parameters.is_empty()
        || !main.parameters.is_empty()
        || main.return_type != ast::Type::I32
        || main.body.is_none()
    {
        return Err("executable entry point must be `fn main(): i32`".to_owned());
    }
    let resolved = semantic::analyze(&program).map_err(|error| error.to_string())?;
    let ir = codegen::generate(&resolved).map_err(|error| error.to_string())?;
    Ok((ir, loaded.headers))
}

#[cfg(test)]
mod tests {
    use super::compile;

    #[test]
    fn compiles_integer_program() {
        let source = r#"
fn main(): i32 {
    let forty: i32 = 40;
    let two = 2;
    return forty + two;
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("store i32 40"));
        assert!(ir.contains("store i32 2"));
        assert!(ir.contains("add i32"));
        assert!(ir.contains("ret i32"));
    }

    #[test]
    fn accepts_export_visibility_on_functions() {
        let source = r#"
export fn answer(): i32 { return 42; }
fn main(): i32 { return answer(); }
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @answer()"));
        assert!(ir.contains("call i32 @answer()"));
    }

    #[test]
    fn observes_operator_precedence() {
        let ir = compile("fn main(): i32 { return 2 + 3 * 4; }").unwrap();
        let multiply = ir.find("mul i32 3, 4").unwrap();
        let add = ir.find("add i32 2,").unwrap();
        assert!(multiply < add);
    }

    #[test]
    fn compiles_function_parameters_and_calls() {
        let source = r#"
fn add(left: i32, right: i32): i32 {
    return left + right;
}

fn main(): i32 {
    return add(40, 2);
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @add(i32 %left, i32 %right)"));
        assert!(ir.contains("call i32 @add(i32 40, i32 2)"));
    }

    #[test]
    fn rejects_wrong_argument_count() {
        let source = "fn id(value: i32): i32 { return value; } fn main(): i32 { return id(); }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("expects 1 arguments, got 0"));
    }

    #[test]
    fn rejects_unknown_names() {
        let error = compile("fn main(): i32 { return missing; }").unwrap_err();
        assert!(error.message.contains("unknown name `missing`"));
    }

    #[test]
    fn rejects_missing_return() {
        let error = compile("fn main(): i32 { let x = 1; }").unwrap_err();
        assert!(error.message.contains("must return on every path"));
    }

    #[test]
    fn compiles_comparison_and_if_else() {
        let source = r#"
fn max(left: i32, right: i32): i32 {
    if left > right {
        return left;
    } else {
        return right;
    }
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("icmp sgt i32"));
        assert!(ir.contains("label %then.0, label %else.1"));
    }

    #[test]
    fn compiles_bool_functions() {
        let source = r#"
fn less(left: i32, right: i32): bool {
    return left < right;
}

fn choose(condition: bool): i32 {
    if condition { return 42; }
    return 0;
}

fn main(): i32 {
    return choose(less(1, 2));
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i1 @less"));
        assert!(ir.contains("call i1 @less"));
        assert!(ir.contains("define i32 @choose(i1 %condition)"));
    }

    #[test]
    fn rejects_non_bool_condition() {
        let error = compile("fn main(): i32 { if 1 { return 1; } return 0; }").unwrap_err();
        assert!(error.message.contains("expected bool, got i32"));
    }

    #[test]
    fn rejects_incomplete_return_paths() {
        let source = "fn main(): i32 { if true { return 42; } }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("must return on every path"));
    }

    #[test]
    fn compiles_assignment_and_for_loop() {
        let source = r#"
fn main(): i32 {
    let total = 0;
    for let value = 0; value < 42; value = value + 1 {
        total = total + value;
    }
    return total;
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("alloca i32"));
        assert!(ir.contains("for.condition.0:"));
        assert!(ir.contains("for.body.1:"));
        assert!(ir.contains("for.update.2:"));
        assert!(ir.contains("br label %for.condition.0"));
    }

    #[test]
    fn rejects_assignment_type_mismatch() {
        let source = "fn main(): i32 { let value = 1; value = false; return value; }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("expected i32, got bool"));
    }

    #[test]
    fn keeps_loop_declarations_scoped() {
        let source = "fn main(): i32 { for ; false; { let inside = 1; } return inside; }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("unknown name `inside`"));
    }

    #[test]
    fn allows_empty_for_clauses() {
        let ir = compile("fn main(): i32 { for ; ; { return 0; } }").unwrap();
        assert!(ir.contains("for.body"));
    }

    #[test]
    fn scopes_for_initializer_bindings() {
        let source = "fn main(): i32 { for let value = 0; false; { return value; } return value; }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("unknown name `value`"));
    }

    #[test]
    fn rejects_moving_linear_value_in_for_condition() {
        let source = r#"
fn main(consume: fn(linear<bool>): bool, value: linear<bool>): i32 {
    for ; consume(value); { return 0; }
    return 0;
}
"#;
        let error = compile(source).unwrap_err();
        assert!(
            error
                .message
                .contains("for condition cannot move a linear value")
        );
    }

    #[test]
    fn compiles_linear_moves() {
        let source = r#"
fn pass(value: linear<i32>): linear<i32> {
    return value;
}

fn relay(value: linear<i32>): linear<i32> {
    let next = pass(value);
    return next;
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @pass(i32 %value)"));
        assert!(ir.contains("call i32 @pass"));
    }

    #[test]
    fn shadows_a_consumed_linear_value_with_its_returned_state() {
        let source = r#"
fn write(value: linear<pointer<u8>>, byte: u8): linear<pointer<u8>> {
    return value;
}
fn update(value: linear<pointer<u8>>): linear<pointer<u8>> {
    let value = write(value, 42u8);
    return value;
}
"#;
        compile(source).unwrap();
    }

    #[test]
    fn rejects_second_use_after_move() {
        let source = r#"
fn pass(value: linear<i32>): linear<i32> { return value; }
fn invalid(value: linear<i32>): linear<i32> {
    let next = pass(value);
    return value;
}
"#;

        let error = compile(source).unwrap_err();
        assert!(error.message.contains("`value` was already moved"));
    }

    #[test]
    fn rejects_unmoved_linear_value_at_return() {
        let source = "fn invalid(value: linear<i32>): i32 { return 0; }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("`value` is not moved before return"));
    }

    #[test]
    fn rejects_overwriting_available_linear_value() {
        let source = r#"
fn invalid(left: linear<i32>, right: linear<i32>): linear<i32> {
    left = right;
    return left;
}
"#;

        let error = compile(source).unwrap_err();
        assert!(
            error
                .message
                .contains("cannot be overwritten before being moved")
        );
    }

    #[test]
    fn rejects_unmoved_linear_block_local() {
        let source = r#"
fn pass(value: linear<i32>): linear<i32> { return value; }
fn invalid(condition: bool, value: linear<i32>): linear<i32> {
    if condition {
        let local = pass(value);
    }
    return value;
}
"#;

        let error = compile(source).unwrap_err();
        assert!(
            error
                .message
                .contains("`local` leaves scope without being moved")
        );
    }

    #[test]
    fn rejects_extern_declarations_in_source_strings() {
        let source = r#"
extern "C" fn acquire(): linear<pointer<i32>>;
"#;

        let error = compile(source).unwrap_err();
        assert!(error.message.contains("only allowed in .landh files"));
    }

    #[test]
    fn rejects_use_after_c_abi_consumer() {
        let source = r#"
fn invalid(value: linear<pointer<i32>>): linear<pointer<i32>> {
    let moved = value;
    return value;
}
"#;

        let error = compile(source).unwrap_err();
        assert!(error.message.contains("`value` was already moved"));
    }

    #[test]
    fn rejects_binding_void_value() {
        let source = r#"
fn action(): void { return; }
fn invalid(): void {
    let result = action();
    return;
}
"#;

        let error = compile(source).unwrap_err();
        assert!(error.message.contains("void value cannot be bound"));
    }

    #[test]
    fn compiles_tuple_destructuring_and_arrays() {
        let source = r#"
fn pair(): (i32, i32) {
    return (40, 2);
}

fn main(): i32 {
    let (left, right) = pair();
    let values = [left, right];
    return values[0] + values[1];
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define { i32, i32 } @pair()"));
        assert!(ir.contains("extractvalue { i32, i32 }"));
        assert!(ir.contains("[2 x i32]"));
        assert!(ir.contains("getelementptr inbounds [2 x i32]"));
    }

    #[test]
    fn rejects_tuple_pattern_length_mismatch() {
        let source = "fn main(): i32 { let (only, extra) = (1, 2, 3); return only; }";
        let error = compile(source).unwrap_err();
        assert!(
            error
                .message
                .contains("tuple pattern expects 3 elements, got 2")
        );
    }

    #[test]
    fn preserves_linearity_through_tuple_destructuring() {
        let source = r#"
fn reorder(value: (linear<i32>, linear<i32>)): (linear<i32>, linear<i32>) {
    let (left, right) = value;
    return (right, left);
}
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define { i32, i32 } @reorder"));
    }

    #[test]
    fn compiles_sized_integer_types() {
        let source = r#"
fn small(): u8 { return 42u8; }
fn wide(left: u64, right: u64): u64 { return left / right; }
fn signed(left: i64, right: i64): bool { return left < right; }
"#;

        let ir = compile(source).unwrap();
        assert!(ir.contains("define i8 @small()"));
        assert!(ir.contains("udiv i64"));
        assert!(ir.contains("icmp slt i64"));
    }

    #[test]
    fn rejects_out_of_range_integer_literal() {
        let error = compile("fn invalid(): u8 { return 256u8; }").unwrap_err();
        assert!(error.message.contains("does not fit in u8"));
    }

    #[test]
    fn rejects_property_syntax() {
        let error = compile("fn invalid(): i32 { let pair = (1, 2); return pair.0; }").unwrap_err();
        assert!(error.message.contains("unexpected character `.`"));
    }

    #[test]
    fn monomorphizes_generic_function_calls() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn main(): i32 { return identity<i32>(42); }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @identity$i32(i32 %value)"));
        assert!(ir.contains("call i32 @identity$i32(i32 42)"));
        assert!(!ir.contains("@identity("));
    }

    #[test]
    fn compiles_generic_higher_order_function() {
        let source = r#"
fn increment(value: i32): i32 { return value + 1; }
fn apply<T>(value: T, operation: fn(T): T): T { return operation(value); }
fn main(): i32 { return apply<i32>(41, increment); }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("define i32 @apply$i32(i32 %value, i32 (i32)* %operation)"));
        assert!(ir.contains("call i32 %"));
        assert!(ir.contains("call i32 @apply$i32(i32 41, i32 (i32)* @increment)"));
    }

    #[test]
    fn function_values_are_copyable_and_callable() {
        let source = r#"
fn increment(value: i32): i32 { return value + 1; }
fn main(): i32 {
    let operation: fn(i32): i32 = increment;
    let copied = operation;
    operation(0);
    return copied(41);
}
"#;
        let ir = compile(source).unwrap();
        assert_eq!(ir.matches("call i32 %").count(), 2);
    }

    #[test]
    fn rejects_missing_or_wrong_generic_type_argument_counts() {
        let missing =
            compile("fn id<T>(value: T): T { return value; } fn main(): i32 { return id(1); }")
                .unwrap_err();
        assert!(missing.message.contains("expects 1 type arguments, got 0"));

        let wrong = compile(
            "fn id<T>(value: T): T { return value; } fn main(): i32 { return id<i32, i32>(1); }",
        )
        .unwrap_err();
        assert!(wrong.message.contains("expects 1 type arguments, got 2"));
    }

    #[test]
    fn rejects_type_arguments_on_non_generic_calls() {
        let error = compile(
            "fn id(value: i32): i32 { return value; } fn main(): i32 { return id<i32>(1); }",
        )
        .unwrap_err();
        assert!(error.message.contains("expects 0 type arguments, got 1"));
    }

    #[test]
    fn generic_call_syntax_does_not_consume_comparisons() {
        let source = r#"
fn choose(value: i32): i32 {
    if value < 42 { return value; }
    if value > 42 { return 0; }
    return 42;
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("icmp slt i32"));
        assert!(ir.contains("icmp sgt i32"));
    }

    #[test]
    fn discovers_nested_specializations_once() {
        let source = r#"
fn inner<T>(value: T): T { return value; }
fn outer<T>(value: T): T { return inner<T>(value); }
fn main(): i32 { return outer<i32>(outer<i32>(42)); }
"#;
        let ir = compile(source).unwrap();
        assert_eq!(ir.matches("define i32 @outer$i32").count(), 1);
        assert_eq!(ir.matches("define i32 @inner$i32").count(), 1);
    }

    #[test]
    fn treats_generic_parameters_as_linear() {
        let duplicate =
            compile("fn duplicate<T>(value: T): (T, T) { return (value, value); }").unwrap_err();
        assert!(duplicate.message.contains("already moved"));

        let discard = compile("fn discard<T>(value: T): i32 { return 0; }").unwrap_err();
        assert!(discard.message.contains("not moved before return"));
    }

    #[test]
    fn compiles_singleton_tuple_destructuring() {
        let source = "fn main(): i32 { let (value,) = (42,); return value; }";
        let ir = compile(source).unwrap();
        assert!(ir.contains("insertvalue { i32 }"));
        assert!(ir.contains("extractvalue { i32 }"));
    }

    #[test]
    fn rejects_duplicate_names_in_nested_pattern() {
        let source = "fn main(): i32 { let (value, (other, value)) = (1, (2, 3)); return value; }";
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("duplicate name `value`"));
    }

    #[test]
    fn rejects_linear_equality_consumption() {
        let source = r#"
fn invalid(left: linear<pointer<i32>>, right: linear<pointer<i32>>): bool {
    return left == right;
}
"#;
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("linear values cannot be compared"));
    }

    #[test]
    fn checks_array_length_and_constant_indices() {
        let oversized =
            compile("fn invalid(value: array<i32, 18446744073709551616>): i32 { return 0; }")
                .unwrap_err();
        assert!(oversized.message.contains("array length exceeds"));

        let out_of_bounds =
            compile("fn invalid(): i32 { let values = [1, 2]; return values[2]; }").unwrap_err();
        assert!(out_of_bounds.message.contains("out of bounds for length 2"));

        compile("fn valid(): i32 { let values = [1, 2]; return values[1]; }").unwrap();
    }

    #[test]
    fn supports_all_integer_literal_boundaries() {
        let source = r#"
fn i8_min(): i8 { return -128i8; }
fn i8_max(): i8 { return 127i8; }
fn i16_min(): i16 { return -32768i16; }
fn i16_max(): i16 { return 32767i16; }
fn i32_min(): i32 { return -2147483648i32; }
fn i32_max(): i32 { return 2147483647i32; }
fn i64_min(): i64 { return -9223372036854775808i64; }
fn i64_max(): i64 { return 9223372036854775807i64; }
fn u8_max(): u8 { return 255u8; }
fn u16_max(): u16 { return 65535u16; }
fn u32_max(): u32 { return 4294967295u32; }
fn u64_max(): u64 { return 18446744073709551615u64; }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("ret i8 -128"));
        assert!(ir.contains("ret i64 -9223372036854775808"));
        assert!(ir.contains("ret i64 18446744073709551615"));
    }

    #[test]
    fn rejects_invalid_integer_operations() {
        let division = compile("fn invalid(): i32 { return 1 / 0; }").unwrap_err();
        assert!(division.message.contains("division by zero"));

        let unsigned_negation = compile("fn invalid(): u8 { return -1u8; }").unwrap_err();
        assert!(
            unsigned_negation
                .message
                .contains("negation requires a signed integer")
        );

        let unknown_suffix = compile("fn invalid(): i32 { return 1wat; }").unwrap_err();
        assert!(
            unknown_suffix
                .message
                .contains("unknown integer suffix `wat`")
        );
    }

    #[test]
    fn generic_symbols_distinguish_nested_tuple_shapes() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn nested(): ((i32, i32), i32) {
    return identity<((i32, i32), i32)>(((1, 2), 3));
}
fn flat(): (i32, i32, i32) {
    return identity<(i32, i32, i32)>((1, 2, 3));
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("@identity$tuple2_tuple2_i32_i32_i32"));
        assert!(ir.contains("@identity$tuple3_i32_i32_i32"));
    }

    #[test]
    fn compiles_constant_true_terminating_loop() {
        let ir = compile("fn main(): i32 { for ; true; { return 42; } }").unwrap();
        assert!(ir.contains("ret i32 42"));
    }

    #[test]
    fn compiles_compound_generic_function_array_program() {
        let source = r#"
fn increment(value: i32): i32 { return value + 1; }
fn apply<T>(value: T, operation: fn(T): T): T { return operation(value); }
fn main(): i32 {
    let operations = [increment, increment];
    let operation = operations[1];
    let pair = (apply<i32>(20, operation), apply<i32>(20, increment));
    let (left, right) = pair;
    return left + right;
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("@apply$i32"));
        assert!(ir.contains("getelementptr inbounds [2 x i32 (i32)*]"));
        assert!(ir.contains("call i32 %"));
    }

    #[test]
    fn tracks_generic_linear_values_across_branches_and_loops() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn choose<T>(condition: bool, value: T): T {
    if condition { return value; } else { return value; }
}
fn cycle<T>(condition: bool, value: T): T {
    for ; condition; {
        let next = identity<T>(value);
        value = next;
    }
    return choose<T>(condition, value);
}
fn lifecycle(condition: bool, value: linear<pointer<i32>>): linear<pointer<i32>> {
    let value = cycle<linear<pointer<i32>>>(condition, value);
    return value;
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("@cycle$linear_pointer_i32"));
        assert!(ir.contains("@choose$linear_pointer_i32"));
        assert!(ir.contains("define i32* @lifecycle"));
    }

    #[test]
    fn calls_void_returning_function_values() {
        let source = r#"
fn action(): void { return; }
fn run(operation: fn(): void): void {
    operation();
    return;
}
fn main(): i32 {
    run(action);
    return 42;
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("call void %"));
    }

    #[test]
    fn immediately_invokes_returned_function() {
        let source = r#"
fn increment(value: i32): i32 { return value + 1; }
fn get(): fn(i32): i32 { return increment; }
fn main(): i32 { return get()(41); }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("call i32 (i32)* @get()"));
        assert!(ir.contains("call i32 %"));
    }

    #[test]
    fn directly_invokes_function_from_array_index() {
        let source = r#"
fn increment(value: i32): i32 { return value + 1; }
fn main(): i32 {
    let operations = [increment, increment];
    return operations[0](41);
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("getelementptr inbounds [2 x i32 (i32)*]"));
        assert!(ir.contains("call i32 %"));
    }

    #[test]
    fn rejects_calling_non_function_expression() {
        let error = compile("fn main(): i32 { return [41, 42][0](1); }").unwrap_err();
        assert!(error.message.contains("value of type i32 is not callable"));
    }

    #[test]
    fn preserves_generic_calls_with_callable_expressions() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn increment(value: i32): i32 { return value + 1; }
fn main(): i32 { return identity<fn(i32): i32>(increment)(41); }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("@identity$fn1_i32_to_i32"));
        assert!(ir.contains("call i32 %"));
    }

    #[test]
    fn local_callable_shadows_generic_global_without_specialization() {
        let source = r#"
fn generic<T>(value: T): T { return value; }
fn increment(value: i32): i32 { return value + 1; }
fn main(): i32 {
    let generic: fn(i32): i32 = increment;
    return generic(41);
}
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("call i32 %"));
        assert!(!ir.contains("@generic$"));
    }

    #[test]
    fn rejects_type_growing_polymorphic_recursion() {
        let source = r#"
fn grow<T>(): void { grow<(T,)>(); return; }
fn main(): void { grow<i32>(); return; }
"#;
        let error = compile(source).unwrap_err();
        assert!(error.message.contains("polymorphic recursion"));
    }

    #[test]
    fn enforces_finite_specialization_budget() {
        let mut source = String::new();
        for index in 0..257 {
            source.push_str(&format!(
                "fn generic{index}<T>(value: T): T {{ return value; }}\n"
            ));
        }
        source.push_str("fn main(): i32 {\n");
        for index in 0..257 {
            source.push_str(&format!("generic{index}<i32>(0);\n"));
        }
        source.push_str("return 0; }");
        let error = compile(&source).unwrap_err();
        assert!(error.message.contains("specialization limit"));
    }

    #[test]
    fn rejects_excessive_parser_nesting() {
        let unary = format!("fn main(): i32 {{ return {}1; }}", "-".repeat(140));
        assert!(
            compile(&unary)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );

        let parentheses = format!(
            "fn main(): i32 {{ return {}1{}; }}",
            "(".repeat(140),
            ")".repeat(140)
        );
        assert!(
            compile(&parentheses)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );

        let nested_type = format!(
            "fn main(value: {}i32{}): i32 {{ return value; }}",
            "(".repeat(140),
            ")".repeat(140)
        );
        assert!(
            compile(&nested_type)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );

        let pattern = format!(
            "fn main(): i32 {{ let {}value{} = 1; return 0; }}",
            "(".repeat(140),
            ")".repeat(140)
        );
        assert!(
            compile(&pattern)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );

        let nested_if = format!(
            "fn main(): i32 {{ {}return 0;{} }}",
            "if true { ".repeat(140),
            " }".repeat(140)
        );
        assert!(
            compile(&nested_if)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );

        let nested_for = format!(
            "fn main(): i32 {{ {}return 0;{} }}",
            "for ; true; { ".repeat(140),
            " }".repeat(140)
        );
        assert!(
            compile(&nested_for)
                .unwrap_err()
                .message
                .contains("nesting depth")
        );
    }

    #[test]
    fn does_not_emit_specializations_from_dead_false_loops() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn main(): i32 {
    for ; false; { identity<i32>(0); }
    return 42;
}
"#;
        let ir = compile(source).unwrap();
        assert!(!ir.contains("@identity$i32"));
    }

    #[test]
    fn rejects_void_in_nested_value_types() {
        for source in [
            "fn accept(operation: fn(void): i32): i32 { return 0; }",
            "fn invalid(value: (i32, void)): i32 { return 0; }",
            "fn invalid(value: array<void, 1>): i32 { return 0; }",
            "fn invalid(): fn(i32): (void, i32) { return invalid; }",
        ] {
            let error = compile(source).unwrap_err();
            assert!(error.message.contains("void cannot be used"), "{error}");
        }
    }

    #[test]
    fn malformed_generic_call_reports_unknown_type() {
        let error = compile(
            "fn id<T>(value: T): T { return value; } fn main(): i32 { return id<unknown>(1); }",
        )
        .unwrap_err();
        assert!(error.message.contains("unknown type `unknown`"));
    }

    #[test]
    fn comparison_shape_is_not_misparsed_as_generic_call() {
        let error = compile("fn main(a: i32, b: i32): bool { return a < b > (1); }").unwrap_err();
        assert!(!error.message.contains("unknown type `b`"));
        assert!(error.message.contains("expected bool, got i32"));
    }

    #[test]
    fn local_shadow_of_function_can_still_use_comparison() {
        let source = r#"
fn value(input: i32): i32 { return input; }
fn main(value: i32): bool { return value < 42; }
"#;
        let ir = compile(source).unwrap();
        assert!(ir.contains("icmp slt i32"));
    }

    #[test]
    fn generic_arguments_cannot_cross_other_postfix_operations() {
        let source = r#"
fn identity<T>(value: T): T { return value; }
fn main(): i32 { return identity<i32>[0](42); }
"#;
        let error = compile(source).unwrap_err();
        assert!(
            error
                .message
                .contains("generic type arguments must be followed by a call")
        );
    }

    #[test]
    fn declaration_errors_precede_body_errors() {
        let duplicate_function =
            compile("fn first(): i32 { return missing; } fn first(): i32 { return 0; }")
                .unwrap_err();
        assert!(duplicate_function.message.contains("duplicate function"));

        let duplicate_parameter = compile(
            "fn first(): i32 { return missing; } fn second(value: i32, value: i32): i32 { return value; }",
        )
        .unwrap_err();
        assert!(duplicate_parameter.message.contains("duplicate parameter"));
    }
}

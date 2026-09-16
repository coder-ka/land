use std::path::Path;

#[test]
fn compiles_the_modules_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/modules/main.land");
    let ir = land::compile_file(&root).unwrap();

    assert!(ir.contains("define i32 @main()"));
    assert!(ir.contains("define i32 @__land_module_0_add(i32 %left, i32 %right)"));
    assert!(ir.contains("define i32 @__land_module_0_forty_two()"));
    assert!(ir.contains("call i32 @__land_module_0_add("));
    assert!(ir.contains("call i32 @__land_module_0_forty_two()"));
    assert!(ir.contains("add i32"));
}

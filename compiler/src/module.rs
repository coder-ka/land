use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::CompileError;
use crate::ast::Program;

pub struct LoadedProgram {
    pub program: Program,
    pub headers: Vec<PathBuf>,
}

pub fn load(entry: &Path) -> Result<LoadedProgram, String> {
    let entry = canonicalize_source(entry)?;
    if source_kind(&entry)? != SourceKind::Land {
        return Err("the entry file must have a .land extension".to_owned());
    }
    let root = entry
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", entry.display()))?
        .to_owned();
    let mut loader = Loader {
        root,
        programs: HashMap::new(),
        order: Vec::new(),
        loading: HashSet::new(),
    };
    loader.visit(&entry)?;
    let headers = loader
        .order
        .iter()
        .filter(|path| source_kind(path) == Ok(SourceKind::LandHeader))
        .cloned()
        .collect();
    let program = loader.combine(&entry)?;
    Ok(LoadedProgram { program, headers })
}

struct Loader {
    root: PathBuf,
    programs: HashMap<PathBuf, Program>,
    order: Vec<PathBuf>,
    loading: HashSet<PathBuf>,
}

impl Loader {
    fn visit(&mut self, path: &Path) -> Result<(), String> {
        if self.programs.contains_key(path) {
            return Ok(());
        }
        if !self.loading.insert(path.to_owned()) {
            return Err(format!("circular import involving {}", path.display()));
        }

        let source = fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let tokens = crate::lexer::lex(&source).map_err(|error| source_error(path, error))?;
        let program = crate::parser::parse(tokens).map_err(|error| source_error(path, error))?;
        validate_source(path, &program)?;
        for import in &program.imports {
            let dependency = self.resolve(path, &import.module)?;
            self.visit(&dependency)?;
        }

        self.loading.remove(path);
        self.order.push(path.to_owned());
        self.programs.insert(path.to_owned(), program);
        Ok(())
    }

    fn resolve(&self, importer: &Path, module: &str) -> Result<PathBuf, String> {
        let requested = Path::new(module);
        if requested.is_absolute() {
            return Err(format!(
                "absolute import paths are not supported: `{module}`"
            ));
        }
        let base = if module.starts_with("./") || module.starts_with("../") {
            importer.parent().unwrap_or(&self.root)
        } else {
            &self.root
        };
        let mut path = base.join(requested);
        if path.extension().is_none() {
            path.set_extension("land");
        }
        canonicalize_source(&path)
    }

    fn combine(mut self, entry: &Path) -> Result<Program, String> {
        let mut names = HashMap::new();
        let mut exports = HashMap::new();
        for (module_id, path) in self.order.iter().enumerate() {
            let program = &self.programs[path];
            let mut module_names = HashMap::new();
            let mut module_exports = HashMap::new();
            for function in &program.functions {
                let symbol = if path == entry || function.body.is_none() {
                    function.name.clone()
                } else {
                    format!("__land_module_{module_id}_{}", function.name)
                };
                module_names.insert(function.name.clone(), symbol.clone());
                if function.exported {
                    module_exports.insert(function.name.clone(), symbol);
                }
            }
            names.insert(path.clone(), module_names);
            exports.insert(path.clone(), module_exports);
        }

        let mut functions = Vec::new();
        for path in &self.order {
            let program = self.programs.remove(path).unwrap();
            let own_names = &names[path];
            let mut visible = own_names.clone();
            for import in &program.imports {
                let dependency = self.resolve(path, &import.module)?;
                let dependency_exports = &exports[&dependency];
                for imported in &import.names {
                    let target =
                        dependency_exports
                            .get(&imported.imported)
                            .ok_or_else(|| {
                                if names[&dependency].contains_key(&imported.imported) {
                                    format!(
                                        "{}:{}:{}: function `{}` is not exported from module `{}`",
                                        path.display(),
                                        import.span.line,
                                        import.span.column,
                                        imported.imported,
                                        import.module
                                    )
                                } else {
                                    format!(
                                        "{}:{}:{}: module `{}` has no function `{}`",
                                        path.display(),
                                        import.span.line,
                                        import.span.column,
                                        import.module,
                                        imported.imported
                                    )
                                }
                            })?;
                    if visible
                        .insert(imported.local.clone(), target.clone())
                        .is_some()
                    {
                        return Err(format!(
                            "{}:{}:{}: duplicate visible function `{}`",
                            path.display(),
                            import.span.line,
                            import.span.column,
                            imported.local
                        ));
                    }
                }
            }
            for mut function in program.functions {
                function.visible_functions = Some(visible.clone());
                function.name = own_names[&function.name].clone();
                functions.push(function);
            }
        }
        Ok(Program {
            imports: Vec::new(),
            functions,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Land,
    LandHeader,
}

fn source_kind(path: &Path) -> Result<SourceKind, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("land") => Ok(SourceKind::Land),
        Some("landh") => Ok(SourceKind::LandHeader),
        _ => Err(format!(
            "unsupported source extension for {}; expected .land or .landh",
            path.display()
        )),
    }
}

fn validate_source(path: &Path, program: &Program) -> Result<(), String> {
    match source_kind(path)? {
        SourceKind::Land => {
            if let Some(function) = program
                .functions
                .iter()
                .find(|function| function.body.is_none())
            {
                return Err(format!(
                    "{}:{}:{}: extern declarations are only allowed in .landh files",
                    path.display(),
                    function.span.line,
                    function.span.column
                ));
            }
        }
        SourceKind::LandHeader => {
            if let Some(import) = program.imports.first() {
                return Err(format!(
                    "{}:{}:{}: imports are not allowed in .landh files",
                    path.display(),
                    import.span.line,
                    import.span.column
                ));
            }
            if let Some(function) = program
                .functions
                .iter()
                .find(|function| function.body.is_some())
            {
                return Err(format!(
                    "{}:{}:{}: .landh files may only contain extern declarations",
                    path.display(),
                    function.span.line,
                    function.span.column
                ));
            }
        }
    }
    Ok(())
}

fn canonicalize_source(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("cannot resolve {}: {error}", path.display()))
}

fn source_error(path: &Path, error: CompileError) -> String {
    format!("{}:{error}", path.display())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::compile_file;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("land-import-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write(&self, name: &str, source: &str) -> PathBuf {
            let path = self.root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, source).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn compiles_relative_imports_with_private_dependencies() {
        let fixture = Fixture::new();
        fixture.write(
            "math.land",
            "fn one(): i32 { return 1; } export fn add_one(value: i32): i32 { return value + one(); }",
        );
        let main = fixture.write(
            "main.land",
            "import { add_one } from \"./math\"; fn main(): i32 { return add_one(41); }",
        );

        let ir = compile_file(main).unwrap();
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("call i32 @__land_module_0_add_one(i32 41)"));
        assert!(ir.contains("call i32 @__land_module_0_one()"));
    }

    #[test]
    fn compiles_bare_generic_imports_from_the_entry_directory() {
        let fixture = Fixture::new();
        fixture.write(
            "nested/identity.land",
            "export fn identity<T>(value: T): T { return value; }",
        );
        let main = fixture.write(
            "nested/main.land",
            "import { identity } from \"identity\"; fn main(): i32 { return identity<i32>(42); }",
        );

        let ir = compile_file(main).unwrap();
        assert!(ir.contains("call i32 @__land_module_0_identity"));
        assert!(ir.contains("i32 42)"));
    }

    #[test]
    fn aliases_named_imports() {
        let fixture = Fixture::new();
        fixture.write("answer.land", "export fn value(): i32 { return 42; }");
        let main = fixture.write(
            "main.land",
            "import { value as answer } from \"./answer\"; fn main(): i32 { return answer(); }",
        );

        let ir = compile_file(main).unwrap();
        assert!(ir.contains("call i32 @__land_module_0_value()"));
    }

    #[test]
    fn imports_c_abi_declarations_from_land_header() {
        let fixture = Fixture::new();
        fixture.write(
            "resource.landh",
            "extern \"C\" fn acquire(): linear<pointer<i32>>; extern \"C\" fn release(value: linear<pointer<i32>>): void;",
        );
        let main = fixture.write(
            "main.land",
            "import { acquire, release as dispose } from \"./resource.landh\"; fn lifecycle(): void { let value = acquire(); dispose(value); return; }",
        );

        let ir = compile_file(main).unwrap();
        assert!(ir.contains("declare i32* @acquire()"));
        assert!(ir.contains("declare void @release(i32* %value)"));
        assert!(ir.contains("call void @release"));
    }

    #[test]
    fn enforces_land_header_contents_and_entry_extension() {
        let fixture = Fixture::new();
        let body = fixture.write("invalid.landh", "fn value(): i32 { return 42; }");
        assert!(
            compile_file(body)
                .unwrap_err()
                .contains("entry file must have a .land extension")
        );

        fixture.write("bad.landh", "fn value(): i32 { return 42; }");
        let imports_body = fixture.write(
            "imports-body.land",
            "import { value } from \"./bad.landh\"; fn main(): i32 { return value(); }",
        );
        assert!(
            compile_file(imports_body)
                .unwrap_err()
                .contains("may only contain extern declarations")
        );

        fixture.write(
            "bad-import.landh",
            "import { value } from \"./bad.landh\"; extern \"C\" fn other(): i32;",
        );
        let imports_header = fixture.write(
            "imports-header.land",
            "import { other } from \"./bad-import.landh\"; fn main(): i32 { return other(); }",
        );
        assert!(
            compile_file(imports_header)
                .unwrap_err()
                .contains("imports are not allowed in .landh files")
        );
    }

    #[test]
    fn rejects_extern_in_land_and_invalid_land_header_abi() {
        let fixture = Fixture::new();
        let direct = fixture.write(
            "direct.land",
            "extern \"C\" fn value(): i32; fn main(): i32 { return value(); }",
        );
        assert!(
            compile_file(direct)
                .unwrap_err()
                .contains("only allowed in .landh files")
        );

        fixture.write("narrow.landh", "extern \"C\" fn narrow(value: u8): u8;");
        let main = fixture.write(
            "narrow-main.land",
            "import { narrow } from \"./narrow.landh\"; fn main(): i32 { return 0; }",
        );
        assert!(
            compile_file(main)
                .unwrap_err()
                .contains("not yet supported at the C ABI boundary")
        );
    }

    #[test]
    fn alias_does_not_expose_the_original_name() {
        let fixture = Fixture::new();
        fixture.write("answer.land", "export fn value(): i32 { return 42; }");
        let main = fixture.write(
            "main.land",
            "import { value as answer } from \"./answer\"; fn main(): i32 { return value(); }",
        );

        assert!(
            compile_file(main)
                .unwrap_err()
                .contains("unknown function `value`")
        );
    }

    #[test]
    fn rejects_missing_and_duplicate_imports() {
        let fixture = Fixture::new();
        fixture.write("values.land", "export fn available(): i32 { return 42; }");
        let missing = fixture.write(
            "missing.land",
            "import { absent } from \"./values\"; fn main(): i32 { return 0; }",
        );
        assert!(
            compile_file(missing)
                .unwrap_err()
                .contains("has no function `absent`")
        );

        let duplicate = fixture.write(
            "duplicate.land",
            "import { available } from \"./values\"; fn available(): i32 { return 0; }",
        );
        assert!(
            compile_file(duplicate)
                .unwrap_err()
                .contains("duplicate visible function `available`")
        );
    }

    #[test]
    fn rejects_circular_imports() {
        let fixture = Fixture::new();
        let first = fixture.write(
            "first.land",
            "import { second } from \"./second\"; export fn first(): i32 { return second(); }",
        );
        fixture.write(
            "second.land",
            "import { first } from \"./first\"; export fn second(): i32 { return first(); }",
        );

        assert!(compile_file(first).unwrap_err().contains("circular import"));
    }

    #[test]
    fn rejects_importing_unexported_function() {
        let fixture = Fixture::new();
        fixture.write("secret.land", "fn secret(): i32 { return 42; }");
        let main = fixture.write(
            "main.land",
            "import { secret } from \"./secret\"; fn main(): i32 { return secret(); }",
        );

        assert!(
            compile_file(main)
                .unwrap_err()
                .contains("is not exported from module")
        );
    }
}

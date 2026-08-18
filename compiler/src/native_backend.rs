use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

type Context = *mut c_void;
type Module = *mut c_void;
type MemoryBuffer = *mut c_void;
type Target = *mut c_void;
type TargetMachine = *mut c_void;
type TargetData = *mut c_void;

unsafe extern "C" {
    fn LLVMInitializeX86TargetInfo();
    fn LLVMInitializeX86Target();
    fn LLVMInitializeX86TargetMC();
    fn LLVMInitializeX86AsmParser();
    fn LLVMInitializeX86AsmPrinter();
    fn LLVMContextCreate() -> Context;
    fn LLVMContextDispose(context: Context);
    fn LLVMCreateMemoryBufferWithMemoryRangeCopy(
        input: *const c_char,
        length: usize,
        name: *const c_char,
    ) -> MemoryBuffer;
    fn LLVMParseIRInContext(
        context: Context,
        buffer: MemoryBuffer,
        module: *mut Module,
        message: *mut *mut c_char,
    ) -> c_int;
    fn LLVMDisposeModule(module: Module);
    fn LLVMDisposeMessage(message: *mut c_char);
    fn LLVMGetDefaultTargetTriple() -> *mut c_char;
    fn LLVMGetTargetFromTriple(
        triple: *const c_char,
        target: *mut Target,
        message: *mut *mut c_char,
    ) -> c_int;
    fn LLVMSetTarget(module: Module, triple: *const c_char);
    fn LLVMCreateTargetMachine(
        target: Target,
        triple: *const c_char,
        cpu: *const c_char,
        features: *const c_char,
        optimization: c_int,
        relocation: c_int,
        code_model: c_int,
    ) -> TargetMachine;
    fn LLVMDisposeTargetMachine(machine: TargetMachine);
    fn LLVMCreateTargetDataLayout(machine: TargetMachine) -> TargetData;
    fn LLVMCopyStringRepOfTargetData(data: TargetData) -> *mut c_char;
    fn LLVMDisposeTargetData(data: TargetData);
    fn LLVMSetDataLayout(module: Module, layout: *const c_char);
    fn LLVMTargetMachineEmitToFile(
        machine: TargetMachine,
        module: Module,
        filename: *mut c_char,
        file_type: c_int,
        message: *mut *mut c_char,
    ) -> c_int;
    fn land_lld_link(argc: c_int, argv: *const *const c_char) -> bool;
}

pub fn compile(ir: &str, output: &Path, links: &[PathBuf]) -> Result<(), String> {
    let files = TemporaryFiles::new()?;
    emit_object(ir, &files.program)?;
    #[cfg(not(target_os = "windows"))]
    emit_object(startup_ir(), &files.startup)?;
    link(&files, output, links)
}

fn emit_object(ir: &str, output: &Path) -> Result<(), String> {
    unsafe {
        LLVMInitializeX86TargetInfo();
        LLVMInitializeX86Target();
        LLVMInitializeX86TargetMC();
        LLVMInitializeX86AsmParser();
        LLVMInitializeX86AsmPrinter();

        let context = LLVMContextCreate();
        let name = c"land";
        let buffer =
            LLVMCreateMemoryBufferWithMemoryRangeCopy(ir.as_ptr().cast(), ir.len(), name.as_ptr());
        let mut module = ptr::null_mut();
        let mut message = ptr::null_mut();
        if LLVMParseIRInContext(context, buffer, &mut module, &mut message) != 0 {
            let error = take_message(message);
            LLVMContextDispose(context);
            return Err(format!("LLVM IR parsing failed: {error}"));
        }

        let triple = LLVMGetDefaultTargetTriple();
        let mut target = ptr::null_mut();
        if LLVMGetTargetFromTriple(triple, &mut target, &mut message) != 0 {
            let error = take_message(message);
            dispose_module(context, module);
            LLVMDisposeMessage(triple);
            return Err(format!("native target initialization failed: {error}"));
        }
        LLVMSetTarget(module, triple);
        let machine =
            LLVMCreateTargetMachine(target, triple, c"generic".as_ptr(), c"".as_ptr(), 2, 0, 0);
        let data = LLVMCreateTargetDataLayout(machine);
        let layout = LLVMCopyStringRepOfTargetData(data);
        LLVMSetDataLayout(module, layout);
        LLVMDisposeMessage(layout);
        LLVMDisposeTargetData(data);

        let filename = CString::new(output.as_os_str().as_encoded_bytes())
            .map_err(|_| "object path contains a null byte".to_owned())?;
        let failed = LLVMTargetMachineEmitToFile(
            machine,
            module,
            filename.as_ptr().cast_mut(),
            1,
            &mut message,
        ) != 0;
        let error = failed.then(|| take_message(message));
        LLVMDisposeTargetMachine(machine);
        LLVMDisposeMessage(triple);
        dispose_module(context, module);
        if let Some(error) = error {
            return Err(format!("object generation failed: {error}"));
        }
    }
    Ok(())
}

fn link(files: &TemporaryFiles, output: &Path, links: &[PathBuf]) -> Result<(), String> {
    let output = output
        .to_str()
        .ok_or_else(|| "output path is not valid UTF-8".to_owned())?;
    let program = files
        .program
        .to_str()
        .ok_or_else(|| "temporary path is not valid UTF-8".to_owned())?;
    #[cfg(target_os = "windows")]
    let mut arguments = vec![
        "land-lld".to_owned(),
        "/entry:main".to_owned(),
        "/subsystem:console".to_owned(),
        "/nodefaultlib".to_owned(),
        format!("/out:{output}"),
        program.to_owned(),
    ];
    #[cfg(not(target_os = "windows"))]
    let mut arguments = vec![
        "land-lld".to_owned(),
        "--entry=_start".to_owned(),
        "--build-id=none".to_owned(),
        "-o".to_owned(),
        output.to_owned(),
        files
            .startup
            .to_str()
            .ok_or_else(|| "temporary path is not valid UTF-8".to_owned())?
            .to_owned(),
        program.to_owned(),
    ];
    arguments.extend(
        links
            .iter()
            .map(|path| {
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "link input path is not valid UTF-8".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let arguments = arguments
        .iter()
        .map(|argument| CString::new(argument.as_str()).unwrap())
        .collect::<Vec<_>>();
    let pointers = arguments
        .iter()
        .map(|argument| argument.as_ptr())
        .collect::<Vec<_>>();
    if unsafe { land_lld_link(pointers.len() as c_int, pointers.as_ptr()) } {
        Ok(())
    } else {
        Err("native linking failed".to_owned())
    }
}

#[cfg(not(target_os = "windows"))]
fn startup_ir() -> &'static str {
    r#"
declare i32 @main()

define void @_start() noreturn {
entry:
  %status32 = call i32 @main()
  %status = sext i32 %status32 to i64
  call void asm sideeffect "syscall", "{rax},{rdi},~{rcx},~{r11},~{memory}"(i64 60, i64 %status)
  unreachable
}
"#
}

unsafe fn take_message(message: *mut c_char) -> String {
    if message.is_null() {
        return "unknown LLVM error".to_owned();
    }
    let result = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    unsafe { LLVMDisposeMessage(message) };
    result
}

unsafe fn dispose_module(context: Context, module: Module) {
    unsafe {
        LLVMDisposeModule(module);
        LLVMContextDispose(context);
    }
}

struct TemporaryFiles {
    program: PathBuf,
    startup: PathBuf,
}

impl TemporaryFiles {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock error: {error}"))?
            .as_nanos();
        let base = std::env::temp_dir().join(format!("landc-{}-{nonce}", std::process::id()));
        Ok(Self {
            program: base.with_extension("program.o"),
            startup: base.with_extension("startup.o"),
        })
    }
}

impl Drop for TemporaryFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.program);
        let _ = fs::remove_file(&self.startup);
    }
}

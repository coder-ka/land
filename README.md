# Land

Land is a programming language that intentionally includes the following among
its general-purpose features:

- Linear types
- C ABI interoperability
- Generics and first-class functions

## Getting Started

Install Land 0.0.1 for Windows x86_64 without administrator privileges:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/coder-ka/land/main/compiler/installers/windows-x86_64.ps1 -OutFile install-land.ps1
./install-land.ps1
landc --version
```

The installer verifies the published SHA-256 checksum, installs `landc.exe` to
`%LOCALAPPDATA%\Land\bin`, and adds that directory to the user `PATH`.

Install Land 0.0.1 for Linux x86_64:

```sh
curl --fail --location https://raw.githubusercontent.com/coder-ka/land/main/compiler/installers/linux-x86_64.sh --output install-land.sh
sh install-land.sh
landc --version
```

The Linux installer places `landc` in `~/.local/bin` and adds that directory to
`~/.profile` when necessary.

Compile a program:

```powershell
landc examples/answer.land -o answer.exe
./answer.exe
```

### Building The Compiler

Build the Linux x86_64 compiler natively:

```sh
./compiler/scripts/build-linux-x86_64.sh
```

Building requires Rust, a C++ compiler, and static LLVM/LLD development
libraries. Set `LLVM_CONFIG` if `llvm-config` has a nonstandard name. The
compiler is written to `compiler/dist/landc-linux-x86_64`; it invokes no
external compiler, LLVM, or linker commands. Standard Linux runtime libraries
are still required.

Build the standalone Windows x86_64 compiler from PowerShell:

```powershell
./compiler/scripts/build-windows-x86_64.ps1
```

This build requires Rust, Git, Visual Studio C++ Build Tools, and network
access. The script downloads pinned LLVM/LLD development archives and builds
their static libxml2 dependency in a local cache. The only distributed artifact
is `compiler/dist/landc-windows-x86_64.exe`; it invokes no external compiler,
LLVM, linker, or SDK commands.

Compile a program with the Linux artifact:

```sh
compiler/dist/landc-linux-x86_64 examples/answer.land -o answer
./answer
```

## Specs

### Functions

Functions declare parameter and return types. `void` functions return without a
value.

```land
fn add(left: i32, right: i32): i32 {
    return left + right;
}

fn run(): void {
    add(40, 2);
    return;
}
```

### Types

Land provides the following built-in types:

- Signed integers: `i8`, `i16`, `i32`, `i64`
- Unsigned integers: `u8`, `u16`, `u32`, `u64`
- Boolean: `bool`
- No value: `void`
- Pointer: `pointer<T>`
- Function: `fn(T): U`
- Tuple: `(T, U)`
- Fixed-length array: `array<T, N>`
- Linear value: `linear<T>`

Integer literals default to `i32` and can use a type suffix.

```land
fn values(): i64 {
    let small = 1u8;
    let large = 42i64;
    return large;
}
```

### Bindings

`let` creates a binding. A type annotation is optional. Existing bindings can
be assigned a value of the same type.

```land
fn count(): i32 {
    let value: i32 = 40;
    value = value + 2;
    return value;
}
```

Bindings may shadow earlier bindings. This is particularly useful when a
linear value is consumed by an operation and returned in its next state:

```land
fn write_file(file: linear<pointer<u8>>, value: i32): linear<pointer<u8>> {
    // The implementation consumes file and returns the usable file state.
    return file;
}

fn write_answer(file: linear<pointer<u8>>): linear<pointer<u8>> {
    let file = write_file(file, 42);
    return file;
}
```

### Operators

Integers support `+`, `-`, `*`, `/`, `<`, `<=`, `>`, and `>=`. Integers,
booleans, and pointers support `==` and `!=`.

```land
fn positive(value: i32): bool {
    return value > 0;
}
```

### Control Flow

`if` conditions and conditional `for` loops require `bool` values.

```land
fn clamp(value: i32): i32 {
    if value < 0 {
        return 0;
    } else {
        return value;
    }
}

fn wait(ready: bool): void {
    for ready {
        return;
    }
    return;
}
```

### Tuples

Tuples are constructed with parentheses and unpacked with a `let` pattern.

```land
fn pair(): (i32, i32) {
    return (40, 2);
}

fn answer(): i32 {
    let (left, right) = pair();
    return left + right;
}
```

### Arrays

Array literals have a fixed length and one element type. Constant out-of-range
indices are rejected; dynamic indexing is unchecked.

```land
fn answer(): i32 {
    let values: array<i32, 2> = [40, 2];
    return values[0] + values[1];
}
```

### Generics

Generic calls require all type arguments and are monomorphized.

```land
fn identity<T>(value: T): T {
    return value;
}

fn answer(): i32 {
    return identity<i32>(42);
}
```

Type parameters are treated as linear inside generic functions and must be
moved exactly once.

### Function Values

Functions are copyable values. Land does not currently provide closures or
captured environments.

```land
fn increment(value: i32): i32 {
    return value + 1;
}

fn apply(value: i32, operation: fn(i32): i32): i32 {
    return operation(value);
}

fn answer(): i32 {
    return apply(41, increment);
}
```

### Imports

Imports are named, may use aliases, and must precede function declarations.

```land
import { add, subtract as minus } from "./math";
```

Relative paths resolve from the importing file. Bare module names resolve from
the entry file's directory. A missing `.land` extension is added automatically.
Imports are not re-exported. Default imports are not supported.

### Linear Values

`linear<T>` has the runtime representation of `T`, but must be moved exactly
once. Passing, returning, or rebinding a linear value moves it.

```land
fn pass(value: linear<i32>): linear<i32> {
    return value;
}

fn relay(value: linear<i32>): linear<i32> {
    let next = pass(value);
    return next;
}
```

A moved value cannot be used again, and an available linear value cannot leave
its scope.

Land intentionally has no borrowing mechanism. An operation that needs
borrow-like access to a linear resource consumes the resource argument and
returns the resulting resource state. Callers are encouraged to bind that
result as a new variable, usually by shadowing the consumed name. APIs such as
file reads and writes should follow this consume-and-return convention rather
than introducing an unchecked borrowed handle. When an operation also produces
a value, it should return the resource and value together for tuple
destructuring.

```land
fn read_count(file: linear<pointer<u8>>): (linear<pointer<u8>>, i32) {
    return (file, 42);
}

fn read_answer(file: linear<pointer<u8>>): linear<pointer<u8>> {
    let (file, count) = read_count(file);
    return file;
}
```

### C ABI

C declarations are written in `.landh` files. A `.landh` file may contain only
`extern "C" fn` declarations and cannot be compiled directly.

```land
extern "C" fn acquire(): linear<pointer<i32>>;
extern "C" fn release(value: linear<pointer<i32>>): void;
```

Import declarations from a regular `.land` file:

```land
import { acquire, release as dispose } from "./resource.landh";
```

`.land` files cannot contain `extern` declarations. Native implementations are
selected with a TOML link map. Each imported header must have exactly one
provider in the selected map; a provider may contain multiple ordered linker
inputs.

```toml
[[provider]]
header = "resource.landh"
links = ["build/resource.lib", "build/runtime.lib"]
```

Header and linker-input paths are relative to the link map. Different maps can
select production and test implementations:

```sh
landc main.land -o app --link-map links.toml
```

The C ABI boundary currently supports `i32`, `u32`, `i64`, `u64`, pointers,
function pointers, and `void` returns. `linear<T>` is ABI-equivalent to `T`.
Narrow integers and aggregate values are not yet supported at this boundary.

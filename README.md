# Land

Land is a programming language that intentionally includes the following among
its general-purpose features:

- Linear types
- C ABI interoperability
- Generics and first-class functions

## Getting Started

Build the Linux x86_64 compiler with Docker:

```sh
./compiler/scripts/build-linux-x86_64.sh
```

The compiler is written to `compiler/dist/landc-linux-x86_64`. Building only
requires Docker. The resulting compiler invokes no external compiler, LLVM, or
linker commands; standard Linux runtime libraries are still required.

Compile a program:

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

`.land` files cannot contain `extern` declarations. Linking the implementation
is a separate build step.

The C ABI boundary currently supports `i32`, `u32`, `i64`, `u64`, pointers,
function pointers, and `void` returns. `linear<T>` is ABI-equivalent to `T`.
Narrow integers and aggregate values are not yet supported at this boundary.

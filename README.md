# JVM-Rust

![badge](https://img.shields.io/badge/version-0.1.0-f39f37) ![Rust](https://github.com/Birbe/jvm/workflows/Rust/badge.svg)

___

A pure Rust implementation of the JVM 7 spec, with the main goals being to be able to eventually:

- Run the unmodified OpenJDK 7 Java source
- Run in a WASM environment

There are currently two modes of execution, but they are both highly unstable/incomplete.

- Interpreted mode
- Bytecode -> WASM compilation

In terms of code simplicity, the interpreter wins, but will end up being much slower
than JITed code.

## Roadmap

Classloader status

- [x] Deserialize Java 7 classes
- [ ] Type-checking of the bytecode (!)
- [ ] Cleanup the deserialization code (somewhat done)

Currently, none of the code is type-checked, so only input Java that you trust.

Interpreter roadmap

- [x] Get basic bytecode running in interpreted mode
- [x] Get heap allocation and object creation working
- [ ] Basic stop-the-world garbage collection
- [ ] Get the OpenJDK 7 source to load and run.
  
WASM-Compiler roadmap

- [x] Get basic bytecode compiled to WASM
- [ ] Control flow*
- [ ] Get heap allocation working
- [ ] Basic stop-the-world garbage collection

*: see technical constraints

---

## Library Usage

Interpreted mode

```Rust
let classpath_root = PathBuf::from("path to root of compiled classes");

let vm = VirtualMachine::new(classpath_root);

vm.class_loader.borrow_mut().load_and_link_class("classpath of main class");

vm.spawn_thread(
    String::from("Main thread"), 
    "Main", //classpath
    "main", 
    "([Ljava/lang/String;)V", //method returns void, takes String[]
    vec![ //String arguments
        String::from("String arguments!")
    ]
);

let mut mut_thread = vm.threads.get_mut("Main thread").unwrap();

while mut_thread.get_stack_count() > 0 {
    match mut_thread.step() {
        Ok(_) => {}
        Err(e) => panic!(format!("JVM errored while stepping! Error:\n{:?}", e))
    }
}
```

Or, run the jvm_test package

```
cargo run -p jvm_test -- --mode [Mode]
```

Mode: i (interpreted) OR wasm (compile to wasm and execute with Wasmtime)

---

## Technical Constraints

#### Control flow

Java bytecode uses arbitrary gotoun/jump instructions, which does not directly
map to WASM. WASM uses a static "block" format, where you define sets of instructions,
of which you can then run commands to go to. Control flow will thus take
more time to get working in the WASM compiler than in the interpreted mode,
but it is technically possible.

#### JIT/Interpreted mode

Due to the current lack of stable support for dynamic linking in WASM,
it's not possible to have a mixed execution mode like you can find in the Oracle JVM,
which is able to gather statistics on hotspots within bytecode, and JIT them to native code
to have them run faster. Technically the only benefit of this would potentially be somewhat
smaller memory usage, but this overhead shouldn't matter.
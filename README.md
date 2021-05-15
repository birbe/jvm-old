# JVM-Rust

![badge](https://img.shields.io/badge/version-0.1.0-f39f37) ![Rust](https://github.com/Birbe/jvm/workflows/Rust/badge.svg)

___

A pure Rust implementation of the JVM 7 spec, with the main goals being to be able to eventually:

- Run the unmodified OpenJDK 7 Java source
- Run in a WASM environment

There are currently two modes of execution, but they are both highly incomplete.

- Interpreted mode
- Bytecode 🠖 WASM compilation

The WASM compiler is still under a lot of work, but works with some extremely simple bytecode. Control flow is the
most difficult part, and that part of the code probably needs to be cleaned up a lot.

## Roadmap

#### Classloader status

- [x] Deserialize Java 7 classes
- [ ] Type-checking of the bytecode (!)
- [x] Cleanup the deserialization code

Currently, none of the code is type-checked, so only input Java that you trust.

#### Interpreter roadmap

- [x] Get basic bytecode running in interpreted mode
- [x] Get heap allocation and object creation working
- [ ] Basic stop-the-world garbage collection
- [ ] Get the OpenJDK 7 source to load and run.
  
#### WASM roadmap

- [x] Get basic bytecode compiled to WASM
- [ ] Control flow *
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

## Technical Stuff

#### Control flow

Java bytecode's control flow is much more loose than that of WASM's; WASM uses a method of control flow where all code blocks
must be predefined in the instructions. You can't just arbitrarily jump to an instruction.
Because of this, a control-flow graph is generated that defines where each Java bytecode can jump to, and that is
then converted into WASM.

#### Classloaders / AOT / JIT

WASM does not yet have stable support for dynamic linking, so more work will have to be put in to make dynamic
classloading (such as user-defined classloaders or ASM) work. One method that could be explored is having the JVM
(compiled into WASM) be executed by JavaScript, which could then oversee the compilation of certain
hotspots in the Java code into WASM, and then executing the generated methods in WASM instead of having it be
interpreted. 
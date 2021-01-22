# JVM-Rust
___

This is a JVM written in pure Rust with the goals of being able to be compiled
to WASM, and eventually run the full OpenJDK properly in the browser
in order to run Java applets/applications.

There are currently two modes of execution, but they are both highly incomplete.

- Interpreted mode
- Bytecode -> WASM compilation

## Roadmap

Classloader status

- [x] Deserialize Java 7 classes
- [ ] Cleanup the serialization code

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

## Usage

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

---

## Technical Constraints

#### Control flow

Java bytecode uses arbitrary goto/jump instructions, which does not directly
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
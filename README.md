# JVM-Rust

![badge](https://img.shields.io/badge/version-0.1.0-f39f37) ![Rust](https://github.com/Birbe/jvm/workflows/Rust/badge.svg)

___

A pure Rust implementation of the JVM 7 spec, with the main goals being to be able to eventually:

- Run the OpenJDK 7 Java source
- Have a Cranelift-backed JIT, and an AOT WASM compiler.

## Roadmap

#### Classloader status

- [x] Deserialize Java 7 classes
- [ ] Type-checking of the bytecode (!)
- [x] Cleanup the deserialization code

Currently, none of the code is type-checked, so only input Java that you trust.

#### Roadmap

- [x] Get basic bytecode running
- [x] Get heap allocation and object creation working
- [ ] Improve stability
- [ ] Monitors and error handling
- [ ] Fully implement all bytecode
- [ ] Get the OpenJDK 7 source to load and run.
- [ ] Stop-the-world GC
- [ ] Bytecode metrics
- [ ] Cranelift JIT
- [ ] Concurrent GC
- [ ] Ahead-of-time WASM compiler

---

## Library Usage

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

---
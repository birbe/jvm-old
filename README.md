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
- [x] Get heap allocation and object creation working in interpreted mode
- [ ] Basic stop-the-world garbage collection
- [ ] Get the OpenJDK 7 source to load and run.
  
WASM-Compiler roadmap

- [x] Get basic bytecode compiled to WASM
- [ ] Get heap allocation working
- [ ] Basic stop-the-world garbage collection

---

## Usage

Interpreted mode

```Rust
let classpath_root = PathBuf::from("path to root of compiled classes");

let vm = VirtualMachine::new(classpath_root);

vm.class_loader.borrow_mut().load_and_link_class("classpath of main class");

let thread = vm.spawn_thread(
    String::from("Main thread"), 
    "main", 
    "([Ljava/lang/String;)V", 
    main.clone(), //Reference to the class
    vec![ //String arguments
        String::from("String arguments!")
    ]
);
```
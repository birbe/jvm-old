# JVM-Rust
___

This is a JVM written in pure Rust with the goals of being able to be compiled
to WASM, and eventually run the full OpenJDK properly in the browser
in order to run Java applets/applications.

There are currently two modes of execution, but they are both highly incomplete.

- Interpreted mode (some bytecodes implemented, not in active development)
- Bytecode -> WASM compilation (in active development)

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

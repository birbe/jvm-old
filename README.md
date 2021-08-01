# JVM-Rust

![badge](https://img.shields.io/badge/version-0.1.0-f39f37) ![Rust](https://github.com/Birbe/jvm/workflows/Rust/badge.svg)

### Warning! This crate is absurdly unsafe at the moment.

___

A pure Rust implementation of the JVM 7 spec, with the main goals being to be able to eventually:

- Run the OpenJDK 7 Java source
- Have a Cranelift-backed JIT, and an AOT WASM compiler.

## Roadmap

- [x] Deserialize Java 7 classes
- [ ] Type-checking of bytecode (!)


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

Look in main.rs of the jvm_test crate for an example.

```
cargo run -p jvm_test -- --mode <i, stepper>
```

---

## Unsafety

This library is under heavy development, and as such is very unstable and unsafe(!) to use. The main issues arise from
the fact that executed bytecode is not currently type-checked.

TLDR: don't use this for anything important, nor on anything you don't trust.
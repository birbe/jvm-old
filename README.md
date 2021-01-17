# JVM-Rust
___

This is a JVM written in pure Rust with the goals of being able to be compiled
to WASM, and eventually run the full OpenJDK properly in the browser
in order to run Java applets/applications.

There are currently two modes of execution, but they are both highly incomplete.

- Interpreted mode (some bytecodes implemented, not in active development)
- Bytecode -> WASM compilation (in active development, not working)
# JVM-Rust
___

This is a JVM written in Rust with the goals of being able to be compiled
to WASM, and eventually run the full OpenJDK properly in the browser
in order to run Java applets/applications.

There are currently two modes of execution, but they are both extremely unstable.

- Interpreted mode (some bytecodes implemented)
- Bytecode -> WASM compilation (does not work)
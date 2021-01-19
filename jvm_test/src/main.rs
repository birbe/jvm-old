use jvm;

use std::fs;
use std::env::current_dir;

use std::time::SystemTime;
use jvm::vm::vm::VirtualMachine;
use std::path::PathBuf;
use wasm::WasmEmitter;
use jvm::vm::linker::loader::ClassLoader;
use wasmtime::{Store, Module, Func, ValType, Instance, Val, MemoryType, Limits};
use wasmtime::Extern::Memory;
use std::io::Write;
use clap::{App, Arg};

fn main() {
    let dir = current_dir().unwrap();

    let javaroot = fs::canonicalize(
        dir.join("jvm_test").join("java")
    ).unwrap();

    let app = App::new("Rust JVM Test")
        .version("0.1")
        .about("Tests the JVM in either WASM-compilation mode or interpreted mode")
        .arg(Arg::with_name("mode")
            .long("mode")
            .short("m")
            .help("Modes: `wasm` or `i` (interpreted)")
            .takes_value(true)
            .required(true))
        .get_matches();

    let mode = app.value_of("mode").unwrap();

    let interpret_or_jit = if mode == "i" {
        true
    } else if mode == "wasm" {
        false
    } else {
        panic!("Unknown execution method. Must be `wasm` or `i` (interpreted)");
    };

    if !interpret_or_jit {
        let mut cl = ClassLoader::new(javaroot);

        let (_, class) = cl.load_and_link_class("Main").ok().unwrap();

        println!("Creating WASM");

        let mut wasm = WasmEmitter::new(&cl, "Main!main!([Ljava/lang/String;)V");

        wasm.process_classes();

        let (wasm, memory) = wasm.build();

        println!("{}", String::from_utf8(memory.data.clone()).unwrap());

        fs::write("./jvm_test/wasm_out/out.wasm", &wasm);

        {
            let store = Store::default();

            let module = Module::from_binary(store.engine(), &wasm[..]).unwrap();

            let print_int = Func::wrap(&store, |x: i32| {
                println!("{}", x);
            });

            let print_string = Func::wrap(&store, |x: i32| {
                println!("String location: {}", x);
            });

            let imports = [print_int.into(), print_string.into()];

            let instance = Instance::new(&store, &module, &imports).unwrap();

            let heap = instance.get_export("heap").unwrap().into_memory().unwrap();

            unsafe {
                heap.data_unchecked_mut().write(&memory.data[..]);
            }

            instance.get_export("main").unwrap().into_func().unwrap().call(
                &[Val::I32(100)]
            );
        }
    } else {
        run_vm(javaroot);
    }
}

fn run_vm(path: PathBuf) {
    let mut vm = VirtualMachine::new(path);

    let start;

    {
        let (_, main) = vm.class_loader.borrow_mut().load_and_link_class("Main").ok().unwrap();

        vm.start_time = SystemTime::now();

        let thread = vm.spawn_thread(String::from("Main"), "main", "([Ljava/lang/String;)V", main.clone(), vec![
            String::from("Hello world!")
        ]);

        let mut mut_thread = vm.threads.get_mut("Main").unwrap();

        start = SystemTime::now();

        while mut_thread.get_stack_count() > 0 {
            mut_thread.step();
        }
    }

    println!("\n[Execution has finished.]\nTime elapsed: {}ms\nObject count: {}", SystemTime::now().duration_since(start).unwrap().as_millis(), vm.heap.borrow().objects.len());
}

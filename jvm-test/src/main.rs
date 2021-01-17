use jvm;

use std::fs;
use std::env::current_dir;

use std::time::SystemTime;
use jvm::vm::vm::VirtualMachine;
use std::path::PathBuf;
use wasm::WasmEmitter;
use jvm::vm::linker::loader::ClassLoader;

fn main() {
    let dir = current_dir().unwrap();

    let normalized = fs::canonicalize(
        dir.join("jvm-test").join("java")
    ).unwrap();

    let interpret_or_jit = false;

    if !interpret_or_jit {
        let mut cl = ClassLoader::new(normalized);

        let (_, class) = cl.load_and_link_class("Main").ok().unwrap();

        println!("Creating WASM");

        let mut wasm = WasmEmitter::new(&cl, "Main!main!([Ljava/lang/String;)V");

        wasm.process_classes();

        println!("Method function map:\n\n{:?}", wasm.method_function_map);

        let bytes = wasm.build();

        fs::write("./out.wasm", bytes);
    } else {
        run_vm(normalized);
    }
}

fn run_vm(path: PathBuf) {
    let mut vm = VirtualMachine::new(path);

    let start;

    {
        let (_, main) = vm.class_loader.borrow_mut().load_and_link_class("Main").ok().unwrap();

        vm.start_time = SystemTime::now();

        let thread = vm.spawn_thread(String::from("Main"), "main", "([Ljava/lang/String;)V", main.clone(), vec![
            //JVM arguments go here
        ]);

        let mut mut_thread = vm.threads.get_mut("Main").unwrap();

        start = SystemTime::now();

        while mut_thread.get_stack_count() > 0 {
            mut_thread.step();
        }
    }

    println!("\n[Execution has finished.]\nTime elapsed: {}ms\nObject count: {}", SystemTime::now().duration_since(start).unwrap().as_millis(), vm.heap.borrow().objects.len());
}

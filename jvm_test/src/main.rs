use jvm;

use std::fs;
use std::env::current_dir;

use std::time::{SystemTime, UNIX_EPOCH};
use jvm::vm::vm::VirtualMachine;
use std::path::PathBuf;
use wasm::WasmEmitter;
use jvm::vm::linker::loader::ClassLoader;
use wasmtime::{Store, Module, Func, ValType, Instance, Val, MemoryType, Limits, ImportType, Caller};
use wasmtime::Memory;

use std::io::{Write, Error};
use clap::{App, Arg};
use byteorder::{BigEndian, ByteOrder, LittleEndian};

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

        fs::write("./jvm_test/wasm_out/out.wasm", &wasm);

        {
            let store = Store::default();

            let module = Module::from_binary(store.engine(), &wasm[..]).unwrap();

            let get_time = Func::wrap(&store, || {
                return SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
            });

            let print_long = Func::wrap(&store, |x: i64| {
                println!("Printed long {}", x);
            });

            let print_int = Func::wrap(&store, |x: i32| {
                println!("{}", x);
            });

            let print_string = Func::wrap(&store,|caller: Caller, x: i32| {
                let memory = caller.get_export("heap").unwrap().into_memory().unwrap();

                unsafe {
                    let data = memory.data_unchecked();

                    let str = String::from_utf8(Vec::from(&data[4..100])).unwrap();

                    println!("{}", str);

                    let u = x as usize;
                    let classpath_utf8_ptr = LittleEndian::read_u32(&data[u..u+4]) as usize;
                    let classpath_utf8_len = LittleEndian::read_u32(&data[classpath_utf8_ptr..classpath_utf8_ptr+4]);

                    let char_arr_ptr = LittleEndian::read_u32(&data[u+4..u+8]) as usize;
                    let char_arr_len = LittleEndian::read_u32(&data[char_arr_ptr..char_arr_ptr+4]) as usize;

                    let chars = &data[char_arr_ptr+4..char_arr_ptr + (char_arr_len * 2) + 4];

                    println!("{}", String::from_utf8(Vec::from(chars)).unwrap());
                }
            });

            let mut imports = vec![
                // print_string.into(),
                print_int.into()
            ];

            let instance = Instance::new(&store, &module, &imports).unwrap();

            let heap = instance.get_export("heap").unwrap().into_memory().unwrap();

            let mem_ptr = heap.data_ptr();

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

    let mut steps = 0;

    {
        let (_, main) = vm.class_loader.borrow_mut().load_and_link_class("Main").ok().unwrap();

        let thread = vm.spawn_thread("Main", "Main", "main", "(I)V", vec![
            String::from("Hello world!")
        ]).unwrap();

        let mut mut_thread = vm.threads.get_mut("Main").unwrap();

        vm.start_time = SystemTime::now();
        start = SystemTime::now();

        let mut step_start = SystemTime::now();

        while mut_thread.get_stack_count() > 0 {
            match mut_thread.step() {
                Ok(_) => {}
                Err(e) => panic!(format!("{:?}", e))
            }

            steps += 1;
        }
    }

    let elapsed = SystemTime::now().duration_since(start).unwrap().as_nanos();

    println!("\n[Execution has finished.]\nAverage step time: {}ns\nSteps: {}\nTime elapsed: {}ms\nObject count: {}", elapsed/steps, steps, SystemTime::now().duration_since(start).unwrap().as_millis(), vm.heap.borrow().objects.len());
}

#[cfg(test)]
mod tests {
    use jvm::vm::class::FieldDescriptor;

    #[test]
    fn descriptors() {
        let field_descriptor_str = "Ljava/lang/String;([[[B);";
        let field_descriptor = FieldDescriptor::parse(field_descriptor_str).unwrap();
        assert_eq!(String::from(&field_descriptor), field_descriptor_str);
    }

}
use jvm;

use std::fs;
use std::env::current_dir;

use std::time::SystemTime;

fn main() {
    let dir = current_dir().unwrap();

    let normalized = fs::canonicalize(
        dir.join("java")
    ).unwrap();

    let vm_rc = VirtualMachine::new(normalized);

    let start;

    {
        let mut vm = (*vm_rc).borrow_mut();

        #[allow(non_snake_case)]
        let (_, class_Main) = vm.load_and_link_class("Main");

        vm.start_time = SystemTime::now();

        let thread = VirtualMachine::spawn_thread(vm_rc.clone(), vm, String::from("Main"), "main", "([Ljava/lang/String;)V", class_Main.clone(), vec![
            //JVM arguments go here
        ]);

        let mut mut_thread = (*thread).borrow_mut();

        start = SystemTime::now();

        while mut_thread.get_stack_count() > 0 {
            mut_thread.step();
        }
    }

    let vm = (*vm_rc).borrow();

    println!("\n[Execution has finished.]\nTime elapsed: {}ms\nObject count: {}", SystemTime::now().duration_since(start).unwrap().as_millis(), vm.objects.len());
}

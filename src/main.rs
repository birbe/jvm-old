extern crate jvm;

use jvm::vm::vm::{VirtualMachine, Type};
use std::fs;
use std::env::current_dir;
use std::ops::Deref;
use std::borrow::{BorrowMut, Borrow};
use std::rc::Rc;
use std::cell::RefCell;
use std::process::exit;
use std::time::SystemTime;
use jvm::vm::class::MethodDescriptor;

fn main() {
    let dir = current_dir().unwrap();

    let normalized = fs::canonicalize(
        dir.join("java")
    ).unwrap();

    let vm_rc = VirtualMachine::new(normalized);

    let mut vm = (*vm_rc).borrow_mut();

    vm.load_and_link_class("Main");

    let class = vm.get_class("Main");

    {
        let thread = VirtualMachine::spawn_thread(vm_rc.clone(), vm, String::from("Main"), "main", "([Ljava/lang/String;)V",class.clone(), vec![
            String::from("It appears it is working! Awesome!")
        ]);

        let mut mut_thread = (*thread).borrow_mut();

        let start = SystemTime::now();

        while mut_thread.get_stack_count() > 0 {
            mut_thread.step();
        }

        println!("Time elapsed {}", SystemTime::now().duration_since(start).unwrap().as_millis());

        println!("Execution has finished.");
    }
}

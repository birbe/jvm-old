extern crate jvm;

use jvm::vm::vm::VirtualMachine;
use jvm::vm::class::FieldDescriptor;
use std::path::{PathBuf};
use std::fs;
use std::env::current_dir;

fn main() {
    let dir = current_dir().unwrap();

    let normalized = fs::canonicalize(
        dir.join("java")
    ).unwrap();

    let mut vm = VirtualMachine::new(normalized);

    vm.load_and_link_class("Main");
}

use std::collections::HashMap;
use crate::vm::class::constant::{Constant};
use std::any::Any;

pub struct VirtualMachine {
    threads: Vec<RuntimeThread>
}

impl VirtualMachine {
    pub fn new() -> VirtualMachine {
        VirtualMachine {
            threads: Vec::new()
        }
    }

    // fn get_constant(&self, index: &u16) -> Box<ConstantInfo<dyn InfoTrait>> {
    //     *self.constant_pool.get_key_value(index)
    // }

    fn spawn_thread(&mut self) {
        self.threads.push(RuntimeThread::new());
    }

    fn step_all_threads(&mut self) {
        for thread in &mut self.threads {
            thread.step();
        }
    }
}

pub struct RuntimeThread {
    program_counter: i64, //The Java SE 7 specification requests that the PC is undefined when a native method is called, so I'm going to implement that as -1
    stack: Vec<Frame>,
    constant_pool: Vec<Type>,
}

enum Type {
    Boolean(bool),
    Byte(u8),
    Char(u16),
    Short(i32),
    Int(i32),
    Float(i128),
    Array(Vec<Type>),
    Reference
}

struct Frame {
    local_vars: Vec<Type>,
    op_stack: Vec<Type>
}

impl Frame {
    pub fn new() -> Frame {
        Frame {
            local_vars: Vec::new(),
            op_stack: Vec::new()
        }
    }
}

impl RuntimeThread {
    pub fn new() -> RuntimeThread {
        RuntimeThread {
            program_counter: -1,
            stack: Vec::new(),
            constant_pool: Vec::new()
        }
    }

    fn step(&mut self) {

    }
}
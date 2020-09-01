use crate::vm::class::{ClassInfo, FieldDescriptor};
use std::collections::HashMap;
use crate::vm::linker::loader::load_class;
use std::fs;
use std::path::{PathBuf};
use std::ops::Deref;

pub struct VirtualMachine {
    threads: Vec<RuntimeThread>,
    classes: HashMap<String, ClassInfo>,
    loaded_classes: HashMap<String, bool>,
    classpath_root: PathBuf
}

impl VirtualMachine {
    pub fn new(classpath_root: PathBuf) -> VirtualMachine {
        VirtualMachine {
            threads: Vec::new(),
            classes: HashMap::new(),
            loaded_classes: HashMap::new(),
            classpath_root
        }
    }

    fn spawn_thread(&mut self, name: String) {
        self.threads.push(RuntimeThread::new(name));
    }

    fn step_all_threads(&mut self) {
        for thread in &mut self.threads {
            thread.step();
        }
    }

    pub fn is_class_linked(&self, classpath: &str) -> bool {
        self.loaded_classes.contains_key(classpath)
    }

    pub fn load_and_link_class(&mut self, classpath: &str) {
        if self.is_class_linked(&classpath) { return } //Exit case

        println!("Loading class {}", classpath);

        let split_classpath = classpath.split("/");
        let mut physical_classpath = PathBuf::new();

        for x in split_classpath {
            physical_classpath = physical_classpath.join(x);
        }
        physical_classpath.set_extension("class");

        let real_path = self.classpath_root.join(physical_classpath);

        let bytes = fs::read(real_path).unwrap();

        let class = load_class(bytes);

        self.loaded_classes.insert(String::from(classpath), true);

        for field in class.fields.iter() {
            match &field.descriptor {
                FieldDescriptor::ObjectType(fd_classpath) => self.load_and_link_class(fd_classpath),
                FieldDescriptor::ArrayType(array_type) => {
                    if let FieldDescriptor::ObjectType(fd_classpath) = array_type.field_descriptor.deref() {
                        self.load_and_link_class(&fd_classpath);
                    }
                },
                FieldDescriptor::BaseType(b) => {}
            }
        }

        if !self.is_class_linked(&class.super_class) {
            self.load_and_link_class(&*String::from(&class.super_class));
        }

        self.classes.insert(String::from(classpath), class);
    }
}

pub struct RuntimeThread {
    thread_name: String,
    program_counter: i64, //The Java SE 7 specification requests that the PC is undefined when a native method is called, so I'm going to implement that as -1
    stack: Vec<Frame>
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
    pub fn new(name: String) -> RuntimeThread {
        RuntimeThread {
            thread_name: name,
            program_counter: -1,
            stack: Vec::new()
        }
    }

    fn step(&mut self) {

    }
}
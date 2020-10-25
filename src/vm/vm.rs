use crate::vm::class::{Class, FieldDescriptor, Method, MethodDescriptor, AccessFlags};
use std::collections::HashMap;
use crate::vm::linker::loader::load_class;
use std::{fs, mem};
use std::path::{PathBuf};
use std::ops::{Deref};
use crate::vm::class::attribute::{AttributeItem};
use crate::vm::class::BaseType;
use std::rc::Rc;
use std::cell::{RefCell, RefMut, Ref};
use std::io::{Cursor};
use byteorder::{ReadBytesExt, BigEndian};
use crate::vm::class::constant::Constant;
use std::alloc::Layout;
use std::alloc::alloc;
use std::mem::size_of;
use std::mem::size_of_val;
use core::ptr;
use std::borrow::Borrow;
use std::hint::unreachable_unchecked;

pub enum InternArrayType {
    Char,
    Int,
    Float,
    Long,
    Double,

    NullReference,
    InterfaceReference,
    ClassReference,
    ArrayReference,
    UnknownReference
}

#[repr(C)]
pub struct ArrayHeader<T> {
    pub id: u8,
    pub size: u16,
    pub item: [T; 0]
}

impl InternArrayType {
    pub fn convert_to_u8(t: InternArrayType) -> u8 {
        match t {
            InternArrayType::Char => 0,
            InternArrayType::Int => 1,
            InternArrayType::Float => 2,
            InternArrayType::Long => 3,
            InternArrayType::Double => 4,
            InternArrayType::NullReference => 5,
            InternArrayType::InterfaceReference => 6,
            InternArrayType::ClassReference => 7,
            InternArrayType::ArrayReference => 8,
            InternArrayType::UnknownReference => 9
        }
    }
    
    pub fn from_u8(t: u8) -> InternArrayType {
        match t {
            0 => InternArrayType::Char,
            1 => InternArrayType::Int,
            2 => InternArrayType::Float,
            3 => InternArrayType::Long,
            4 => InternArrayType::Double,
            5 => InternArrayType::NullReference,
            6 => InternArrayType::InterfaceReference,
            7 => InternArrayType::ClassReference,
            8 => InternArrayType::ArrayReference,
            9 => InternArrayType::UnknownReference,
            _ => panic!(format!("Invalid array type [{}] from u8!", t))
        }
    }

    pub fn from_type(t: Type) -> InternArrayType {
        match t {
            Type::Char(_) => InternArrayType::Char,
            Type::Int(_) => InternArrayType::Int,
            Type::Float(_) => InternArrayType::Float,
            Type::LongHalf(_) => InternArrayType::Long,
            Type::DoubleHalf(_) => InternArrayType::Double,
            Type::Reference(r) => {
                match r {
                    Reference::Interface(_) => InternArrayType::InterfaceReference,
                    Reference::Null => panic!("Cannot use null as an array type!"),
                    Reference::Class(_) => InternArrayType::ClassReference,
                    Reference::Array(_) => InternArrayType::ArrayReference
                }
            }
        }
    }
}

pub struct VirtualMachine {
    threads: HashMap<String, Rc<RefCell<RuntimeThread>>>,
    pub classes_map: HashMap<String, Rc<Class>>,
    pub class_info_map: HashMap<String, Rc<Class>>,
    loaded_classes: HashMap<String, bool>,
    strings: HashMap<String, *mut u8>,
    classpath_root: PathBuf
}

impl VirtualMachine {
    pub fn new(classpath_root: PathBuf) -> Rc<RefCell<VirtualMachine>> {
        Rc::new(RefCell::new(VirtualMachine {
            threads: HashMap::new(),
            classes_map: HashMap::new(),
            class_info_map: HashMap::new(),
            loaded_classes: HashMap::new(),
            strings: HashMap::new(),
            classpath_root
        }))
    }

    pub fn get_class(&self, classpath: &str) -> Rc<Class> {
        self.classes_map.get(classpath).unwrap().clone()
    }

    pub fn spawn_thread(rc: Rc<RefCell<VirtualMachine>>, mut vm: RefMut<VirtualMachine>, name: String, method_name: &str, method_descriptor: &str, class: Rc<Class>, args: Vec<String>) -> Rc<RefCell<RuntimeThread>> {

        let mut thread = RuntimeThread::new(String::from(&name), rc);

        let mut frame = RuntimeThread::create_frame(
            method_name,
            method_descriptor,
            &class
        );

        let mut index: u16 = 0;

        let string_arr_ptr = VirtualMachine::allocate_array::<usize>(InternArrayType::ClassReference, args.len());

        let (header, body) = unsafe { VirtualMachine::get_array::<usize>(string_arr_ptr as *mut u8) };

        frame.local_vars.insert(0, Type::Reference(Reference::Array(header as *mut u8)));

        for arg in args.iter() {
            let str = RuntimeThread::create_string(&mut vm, arg);

            unsafe {
                *body.offset(size_of::<usize>() as isize * index as isize) = str as usize;
            }

            index += 1;
        }

        thread.add_frame(
            frame
        );

        vm.threads.insert(String::from(&name),
                          Rc::new(RefCell::new(thread)));
        vm.threads.get(&name).unwrap().clone()

    }

    pub fn is_class_linked(&self, classpath: &str) -> bool {
        self.loaded_classes.contains_key(classpath)
    }

    pub fn load_and_link_class(&mut self, classpath: &str) -> (bool, Rc<Class>) {
        if self.is_class_linked(&classpath) {
            return (
                false,
                self.classes_map.get(classpath).unwrap().clone()
            );
        } //Exit case

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

        for (_, info) in class.field_map.iter() {
            match &info.info.field_descriptor {
                FieldDescriptor::ObjectType(fd_classpath) => { self.load_and_link_class(fd_classpath); },
                FieldDescriptor::ArrayType(array_type) => {
                    if let FieldDescriptor::ObjectType(fd_classpath) = array_type.field_descriptor.deref() {
                        self.load_and_link_class(&fd_classpath);
                    }
                },
                FieldDescriptor::BaseType(_) => {}
            }
        }

        if !self.is_class_linked(&class.super_class) {
            if &class.super_class != "" {
                self.load_and_link_class(&*String::from(&class.super_class));
            }
        }

        self.classes_map.insert(String::from(classpath), Rc::new(class));

        (
            true,
            self.classes_map.get(classpath).unwrap().clone()
        )
    }

    pub fn allocate_class(class: &Rc<Class>) -> *mut u8 {
        unsafe {
            let mut size = 0;

            for (_, info) in class.field_map.iter() {
                match &info.info.field_descriptor {
                    FieldDescriptor::BaseType(base) => {
                        size += BaseType::size_of(base);
                    },
                    FieldDescriptor::ObjectType(_) => {
                        size += size_of::<usize>()
                    },
                    FieldDescriptor::ArrayType(_) => { //Will be a reference to an array object
                        size += size_of::<usize>()
                    }
                }
            }

            alloc(Layout::from_size_align(size, 2).unwrap())
        }
    }

    pub fn put_field<T>(ptr: *mut u8, class: &Rc<Class>, field: &str, value: T) {
        let field_offset = class.field_map.get(
            field
        );

        if field_offset.is_none() {
            for (str, obj_field) in class.field_map.iter() {
                println!("key {}, obj {}", str, obj_field.offset);
            }
        }


        unsafe {
            let offset_ptr = ptr.offset(
                field_offset.unwrap().offset as isize
            ) as (*mut T);

            *offset_ptr = value;
        }
    }

    pub fn get_field<T>(ptr: *mut u8, class: &Rc<Class>, field: &str) -> *mut T {
        unsafe {
            let offset_ptr = ptr.offset(
                class.field_map.get(
                    field
                ).unwrap().offset as isize
            ) as (*mut T);

            offset_ptr
        }
    }

    pub fn allocate_array<T>(intern_type: InternArrayType, length: usize) -> *mut ArrayHeader<T> {
        let id = InternArrayType::convert_to_u8(intern_type);

        let header = Layout::new::<ArrayHeader<T>>();
        let body = Layout::array::<T>(length).unwrap();

        let (layout, offset) = header.extend(body).unwrap();

        assert_eq!(offset, mem::size_of::<ArrayHeader<T>>());
        assert!(length < u16::MAX as usize);

        unsafe {
            let ptr = alloc(layout);

            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }

            let header = ptr.cast::<ArrayHeader<T>>();
            (*header).id = id;
            (*header).size = length as u16;

            ptr.cast::<ArrayHeader<T>>()
        }
    }
    
    pub unsafe fn get_array<T>(ptr: *mut u8) -> (*mut ArrayHeader<T>, *mut T) {
        let header_ptr = ptr.cast::<ArrayHeader<T>>();
        // let body_ptr = header_ptr.offset(size_of::<ArrayHeader<T>>() as isize).cast::<T>();
        let body_ptr = header_ptr.offset(1).cast::<T>();

        (
            header_ptr,
            body_ptr
        )
    }

    pub fn allocate_chars(&mut self, string: &str) -> *mut ArrayHeader<u8> {
        unsafe {
            let header = VirtualMachine::allocate_array::<u8>(InternArrayType::Char, string.len());

            let (mut arr_header, mut arr_body) = VirtualMachine::get_array::<u8>(header as *mut u8);

            ptr::copy(string.as_bytes().as_ptr(), arr_body, string.as_bytes().len());

            arr_header
        }
    }

    pub fn invoke_native(&self, name: &str, class: &Rc<Class>, mut argument_ops: Vec<Operand>) -> Option<Vec<Operand>> {
        let full_name = format!("{}_{}", class.this_class.replace("/","_"),name.to_ascii_lowercase());

        match full_name.as_str() {
            "Main_print_int" => {
                println!("Main_print_int({})", argument_ops.pop().unwrap().1);

                Option::None
            },
            "Main_print_string" => {
                let string_reference = argument_ops.pop().unwrap().1;

                let chars_ptr = VirtualMachine::get_field::<usize>(string_reference as *mut u8, &self.get_class("java/lang/String"), "chars");
                let mut string_bytes: Vec<u8> = Vec::new();

                unsafe {
                    let (header, body) = VirtualMachine::get_array::<u8>(*chars_ptr as *mut u8);
                    let length = (*header).size;

                    for i in 0..length {
                        let char = *(body.offset(
                            // (length as isize - i as isize - 1)
                            (i as isize) * size_of::<u8>() as isize
                        ));
                        string_bytes.push(char);
                    }

                    let str = String::from_utf8(string_bytes).unwrap();
                    println!("{}", str);

                    Option::None
                }
            },
            "Main_get_time" => {

                let ops_out: Vec<Operand> = vec![
                    // Operand(OperandType::LongHalf, )
                ];

                Option::Some(ops_out)
            },
            _ => unimplemented!("Unimplemented native method \"{}\"", full_name)
        }
    }
}

#[derive(Debug)]
pub enum Type {
    Char(u16),
    Int(i32),
    Float(f32),
    LongHalf(u32),
    DoubleHalf(u32),
    Reference(Reference)
}

impl Type {
    pub fn as_operand(t: Type) -> Operand {
        match t {
            Type::Char(c) => Operand(OperandType::Char, c as usize),
            Type::Int(i) => Operand(OperandType::Int, i as usize),
            Type::Float(f) => Operand(OperandType::Float, f as usize),
            Type::LongHalf(h) => Operand(OperandType::LongHalf, h as usize),
            Type::DoubleHalf(h) => Operand(OperandType::DoubleHalf, h as usize),
            Type::Reference(r) => {
                match r {
                    Reference::Class(ptr) => Operand(OperandType::ClassReference, ptr as usize),
                    Reference::Null => Operand(OperandType::NullReference, 0),
                    Reference::Interface(ptr) => Operand(OperandType::InterfaceReference, ptr as usize),
                    Reference::Array(ptr) => Operand(OperandType::ArrayReference, ptr as usize)
                }
            }
        }
    }

    pub fn get_size(t: Type) -> usize {
        match t {
            Type::Char(_) => size_of::<u16>(),
            Type::Int(_) => size_of::<i32>(),
            Type::Float(_) => size_of::<f32>(),
            Type::LongHalf(_) => size_of::<u32>(),
            Type::DoubleHalf(_) => size_of::<u32>(),
            Type::Reference(_) => size_of::<usize>() //Size of a null reference should never be checked, so this is a fair assumption.
        }
    }
}

#[derive(Debug)]
pub enum OperandType {
    Char,
    Int,
    Float,
    LongHalf,
    DoubleHalf,

    NullReference,
    InterfaceReference,
    ClassReference,
    ArrayReference
}

impl OperandType {
    pub fn from_base_type(bt: BaseType) -> Self {
        match bt {
            BaseType::Byte => OperandType::Int,
            BaseType::Char => OperandType::Int,
            BaseType::Double => OperandType::DoubleHalf,
            BaseType::Float => OperandType::Float,
            BaseType::Int => OperandType::Int,
            BaseType::Long => OperandType::LongHalf,
            BaseType::Reference => OperandType::ArrayReference,
            BaseType::Bool => OperandType::Int,
            BaseType::Short => OperandType::Int
        }
    }
}

impl Clone for OperandType {
    fn clone(&self) -> Self {
        match self {
            OperandType::Char => OperandType::Char,
            OperandType::Int => OperandType::Int,
            OperandType::Float => OperandType::Float,
            OperandType::LongHalf => OperandType::LongHalf,
            OperandType::DoubleHalf => OperandType::DoubleHalf,
            OperandType::NullReference => OperandType::NullReference,
            OperandType::InterfaceReference => OperandType::InterfaceReference,
            OperandType::ClassReference => OperandType::ClassReference,
            OperandType::ArrayReference => OperandType::ArrayReference
        }
    }
}

pub struct Operand (OperandType, usize);

impl Clone for Operand {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl Operand {
    pub fn as_type(op: Operand) -> Type {
        match op.0 {
            OperandType::Char => Type::Char(op.1 as u16),
            OperandType::Int => Type::Int(op.1 as i32),
            OperandType::Float => Type::Float(op.1 as f32),
            OperandType::LongHalf => Type::LongHalf(op.1 as u32),
            OperandType::DoubleHalf => Type::DoubleHalf(op.1 as u32),
            OperandType::NullReference => Type::Reference(Reference::Null),
            OperandType::InterfaceReference => Type::Reference(Reference::Interface(op.1 as *mut u8)),
            OperandType::ClassReference => Type::Reference(Reference::Class(op.1 as *mut u8)),
            OperandType::ArrayReference => Type::Reference(
                Reference::Array(op.1 as *mut u8)
            )
        }
    }
}

#[derive(Debug)]
pub enum Reference {
    Null,
    Interface(*mut u8),
    Class(*mut u8),
    Array(*mut u8)
}

pub struct Frame {
    code: Cursor<Vec<u8>>,
    local_vars: HashMap<u16, Type>,
    method_name: String,
    op_stack: Vec<Operand>,
    class: Rc<Class>
}

pub struct RuntimeThread {
    vm: Rc<RefCell<VirtualMachine>>,
    thread_name: String,
    stack: Vec<Frame>
}

impl RuntimeThread {
    pub fn new(name: String, vm: Rc<RefCell<VirtualMachine>>) -> RuntimeThread {
        RuntimeThread {
            vm,
            thread_name: name,
            stack: Vec::new()
        }
    }

    pub fn create_frame(method_name: &str, descriptor: &str, class: &Rc<Class>) -> Frame {
        let method = class.get_method(method_name, descriptor);
        let attr = method.attribute_map.get("Code").unwrap();
        let code_cursor= if let AttributeItem::Code(code) = &attr.info {
            Cursor::new(code.code.clone())
        } else { unreachable!("Code attribute did not resolve to Attribute::Code variant!") };

        Frame {
            class: Rc::clone(class),
            code: code_cursor,
            method_name: String::from(method_name),
            local_vars: HashMap::new(),
            op_stack: Vec::new()
        }
    }

    pub fn add_frame(&mut self, frame: Frame) {
        self.stack.push(frame)
    }

    pub fn create_string(vm: &mut RefMut<VirtualMachine>, string: &str) -> *mut u8 {
        let (_, str_class) = vm.load_and_link_class("java/lang/String");

        if vm.strings.contains_key(string) {
            return *vm.strings.get(string).unwrap();
        }

        let allocated_class = VirtualMachine::allocate_class(&str_class);
        let chars_ptr = VirtualMachine::allocate_chars(vm, string);

        VirtualMachine::put_field::<usize>(
            allocated_class,
            &str_class,
            "chars",
            chars_ptr as usize
        );

        vm.strings.insert(String::from(string), allocated_class);

        allocated_class
    }

    pub fn step(&mut self) {
        let mut pending_frames: Option<Vec<Frame>> = Option::None;

        let frame = self.stack.last_mut().unwrap();

        let mut vm = (&*self.vm).borrow_mut();

        let opcode_pos = frame.code.position();
        let opcode = frame.code.read_u8().unwrap();

        match opcode {
            0x0 => (), //nop, do nothing
            0x1 => frame.op_stack.push(Operand(OperandType::NullReference, 0)), //aconst_null,
            0x2..=0x8 => frame.op_stack.push(Operand(OperandType::Int, (opcode - 0x3) as usize)),
            0x9..=0xa => { //lconst_<n>
                frame.op_stack.push(Operand(OperandType::LongHalf, 0 ));
                frame.op_stack.push(Operand(OperandType::LongHalf, (opcode-0x9) as usize ));
            },
            0xb..=0xd => frame.op_stack.push(Operand(OperandType::Float, (opcode-0xb) as usize)), //fconst_<n>
            0xe..=0xf => { //dconst_<n>
                frame.op_stack.push(Operand(OperandType::DoubleHalf, 0 ));
                frame.op_stack.push(Operand(OperandType::DoubleHalf, (opcode-0xe) as usize));
            },
            0x10 => frame.op_stack.push(Operand(OperandType::Int, frame.code.read_u8().unwrap() as usize)), //bipush
            0x11 => frame.op_stack.push(Operand(OperandType::Int, frame.code.read_u16::<BigEndian>().unwrap() as usize)), //sipush
            ///ldc
            0x12 => { //ldc (load constant)
                let index = frame.code.read_u8().unwrap();
                let constant = frame.class.constant_pool.get(index as usize).unwrap();

                match constant {
                    Constant::Integer(int) => frame.op_stack.push(Operand(OperandType::Int, *int as usize)),
                    Constant::Float(float) => frame.op_stack.push(Operand(OperandType::Float, *float as usize)),
                    Constant::String(str_index) => {
                        let (_, str_class) = vm.load_and_link_class("java/lang/String");

                        if let Constant::Utf8(string) = frame.class.constant_pool.get(*str_index as usize).unwrap() {
                            let allocated_string = RuntimeThread::create_string(
                                &mut vm,
                                string
                            );

                            frame.op_stack.push(Operand(OperandType::ClassReference, allocated_string as usize));
                        }
                    }
                    Constant::Utf8(_) => {}
                    Constant::Long(_) => {}
                    Constant::Double(_) => {}
                    Constant::Class(_) => {}
                    Constant::FieldRef(_, _) => {}
                    Constant::MethodRef(_, _) => {}
                    Constant::InterfaceMethodRef(_, _) => {}
                    Constant::NameAndType(_, _) => {}
                    Constant::MethodHandle(_, _) => {}
                    Constant::MethodType(_) => {}
                    Constant::InvokeDynamic(_, _) => {}
                }
            }
            /////iload_<n> ; Load int from local variables
            0x1a..=0x1d => {
                let local_var = frame.local_vars.get(&((opcode - 0x1a) as u16));

                if local_var.is_none() {
                    println!("Class: {}, Method: {}, Index: {}, Opcode loc: {}", frame.class.this_class, frame.method_name, opcode - 0x1a, frame.code.position()-1);
                    panic!("bruh moment");
                }

                if let Type::Int(int) = local_var.unwrap() { //<n> = 0..3
                    frame.op_stack.push(Operand(OperandType::Int, *int as usize))
                } else { panic!("iload_n command did not resolve to an int!") }
            },
            ///lload_<n> ; Load long from local variables
            0x1e..=0x21 => {
                if let Type::LongHalf(lhalf1) = frame.local_vars.get(&((opcode-0x1e) as u16)).unwrap() { //<n> = 0..3
                    if let Type::LongHalf(lhalf2) = frame.local_vars.get(&((opcode-0x1e) as u16 + 1)).unwrap() { //<n> = 0..3
                        frame.op_stack.push(Operand(OperandType::LongHalf, *lhalf1 as usize));
                        frame.op_stack.push(Operand(OperandType::LongHalf, *lhalf2 as usize));
                    } else { panic!("lload_n command did not resolve to a long!") }
                } else { panic!("lload_n command did not resolve to a long!") }
            },
            ///fload_<n> ; Load float from local variables
            0x22..=0x25 => {
                if let Type::Float(float) = frame.local_vars.get(&((opcode-0x22) as u16)).unwrap() { //<n> = 0..3
                    frame.op_stack.push(Operand(OperandType::Float, *float as usize));
                } else { panic!("fload_n command did not resolve to an float!") }
            },
            ///dload_<n> ; Load double from local variables
            0x26..=0x29 => {
                if let Type::DoubleHalf(dhalf1) = frame.local_vars.get(&((opcode-0x26) as u16)).unwrap() { //<n> = 0..3
                    if let Type::DoubleHalf(dhalf2) = frame.local_vars.get(&((opcode-0x1e) as u16 + 1)).unwrap() { //<n> = 0..3
                        frame.op_stack.push(Operand(OperandType::DoubleHalf, *dhalf1 as usize));
                        frame.op_stack.push(Operand(OperandType::DoubleHalf, *dhalf2 as usize));
                    } else { panic!("dload_n command did not resolve to a double!") }
                } else { panic!("dload_n command did not resolve to a double!") }
            },
            ///aload_<n> ; Load reference
            0x2a..=0x2d => {
                let index = opcode-0x2a;

                let var = frame.local_vars.get(&(index as u16)).unwrap();

                if let Type::Reference(reference) = var {
                    match reference {
                        Reference::Class(ptr) => {
                            frame.op_stack.push(Operand(OperandType::ClassReference, (*ptr) as usize))
                        },
                        Reference::Null => frame.op_stack.push(Operand(OperandType::NullReference, 0)),
                        Reference::Interface(ptr) => frame.op_stack.push(Operand(OperandType::InterfaceReference, *ptr as usize)),
                        Reference::Array(ptr) => frame.op_stack.push(Operand(OperandType::ArrayReference, *ptr as usize))
                    }
                } else { panic!("aload_<n> local variable did not resolve to a reference!") };
            },
            0x30 => { //lconst_f
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;
                let index = frame.op_stack.pop().unwrap().1 as usize;

                unsafe {
                    let float = (arr_ptr.offset((size_of::<f32>() * index) as isize)) as *mut f32;
                    frame.op_stack.push(Operand(OperandType::Float, *float as usize));
                }
            },
            0x31 => { //lconst_d
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;
                let index = frame.op_stack.pop().unwrap().1 as usize;

                unsafe {
                    let double = *((arr_ptr.offset((size_of::<u64>() * index) as isize)) as *mut u64);

                    let dhalf1 = (double >> 32) as u32 as usize;
                    let dhalf2 = double as u32 as usize;

                    frame.op_stack.push(Operand(OperandType::DoubleHalf, dhalf1));
                    frame.op_stack.push(Operand(OperandType::DoubleHalf, dhalf2));
                }
            },
            0x32 => { //aaload (load reference from an array)
                let index = frame.op_stack.pop().unwrap().1 as isize;
                let arr_ptr = frame.op_stack.pop().unwrap().1 as *mut u8;

                let (header, body) = unsafe { VirtualMachine::get_array::<usize>(arr_ptr) };

                let header_id = unsafe { (*header).id };

                println!("header id {}", header_id);

                let ref_type = match InternArrayType::from_u8(header_id) {
                    InternArrayType::ArrayReference => OperandType::ArrayReference,
                    InternArrayType::ClassReference => OperandType::ClassReference,
                    InternArrayType::InterfaceReference => OperandType::InterfaceReference,
                    _ => panic!("Reference in array was not a reference.")
                };

                println!("uh huh");

                let element = unsafe {
                    body.offset(size_of::<usize>() as isize * index)
                };

                println!("element pointer @ {}", element as usize);

                frame.op_stack.push(Operand(ref_type, element as usize));
            },
            ///istore_<n>
            0x3b..=0x3e => {
                let index = (opcode - 0x3b) as u16;
                let value = frame.op_stack.pop().unwrap().1 as i32;
                frame.local_vars.insert(index, Type::Int(value));
            },
            ///astore_<n>
            0x4b..=0x4e => {
                let index = opcode-0x4b;

                let object_ref = frame.op_stack.pop().unwrap();

                frame.local_vars.insert(index as u16, Operand::as_type(object_ref));
            },
            ///dup
            0x59 => {
                let op = frame.op_stack.pop().unwrap();
                frame.op_stack.push(op.clone());
                frame.op_stack.push(op);
            }
            0x60 => {
                let int1 = frame.op_stack.pop().unwrap().1 as i32;
                let int2 = frame.op_stack.pop().unwrap().1 as i32;

                frame.op_stack.push(Operand(OperandType::Int,(int1 + int2) as usize));
            },
            0x84 => { //inc
                let index = frame.code.read_u8().unwrap();
                let byte = frame.code.read_u8().unwrap();

                let var = frame.local_vars.get_mut(&(index as u16)).unwrap();
                if let Type::Int(int) = var {
                    let num = *int + byte as i32;
                    frame.local_vars.insert(byte as u16, Type::Int(num));
                } else { panic!("Local variable used in iinc was not an int!") }
            },
            0x99..=0x9e => {
                let offset = frame.code.read_u16::<BigEndian>().unwrap()- 2;
                let val = frame.op_stack.pop().unwrap().1;

                if match opcode {
                    0x99 => val == 0, //ifeq
                    0x9a => val != 0, //ifne
                    0x9b => val < 0, //iflt
                    0x9c => val >= 0, //iflte
                    0x9d => val > 0, //ifgt
                    0x9e => val >= 0, //ifge
                    _ => unreachable!()
                } {
                    frame.code.set_position(frame.code.position() + offset as u64);
                }
            }
            0x9f..=0xa4 => {
                let offset = frame.code.read_u16::<BigEndian>().unwrap() - 3; //Subtract the two bytes read and the opcode
                //Subtract two because the offset will be used relative to the opcode, not the last byte.

                let i2 = frame.op_stack.pop().unwrap().1 as u32;
                let i1 = frame.op_stack.pop().unwrap().1 as u32;

                let branch = match opcode {
                    0x9f => i1 == i2, //if_icmpeq
                    0xa0 => i1 != i2, //if_icmpne
                    0xa1 => i1 < i2, //if_icmplt
                    0xa2 => i1 >= i2, //if_icmpge
                    0xa3 => i1 > i2, //if_icmpgt
                    0xa4 => i1 <= i2, //if_icmple
                    _ => unreachable!()
                };
                if branch {
                    frame.code.set_position(frame.code.position() + offset as u64);
                }
            },
            0xa7 => { //Goto
                let offset = ( frame.code.read_i16::<BigEndian>().unwrap()) as i64;
                let pos = (frame.code.position() as i64) + offset;
                frame.code.set_position(( pos as u64 ) - 3);
            },
            0xb1 => { //return void
                self.stack.pop();
            },
            ///getfield
            0xb4 => {
                //TODO: type checking, exceptions

                let index = frame.code.read_u16::<BigEndian>().unwrap();

                let fieldref = frame.class.constant_pool.resolve_ref_info(index as usize);

                let class = vm.get_class(&fieldref.class_name);
                let object_ref = frame.op_stack.pop().unwrap().1 as *mut u8;

                let value = VirtualMachine::get_field::<usize>(object_ref, &class, &fieldref.name);
                let fd = FieldDescriptor::parse(&fieldref.descriptor);

                let operand = match fd {
                    FieldDescriptor::BaseType(base_type) => {
                        Operand(OperandType::from_base_type(base_type), unsafe { *value } )
                    },
                    FieldDescriptor::ObjectType(object_type) => {
                        Operand(OperandType::ClassReference, unsafe { *value })
                    },
                    FieldDescriptor::ArrayType(_) => panic!("Not allowed.")
                };

                frame.op_stack.push(operand);
            },
            0xb5 => { //putfield
                //TODO: type checking, exceptions
                let index = frame.code.read_u16::<BigEndian>().unwrap();

                let fieldref = frame.class.constant_pool.resolve_ref_info(index as usize);

                let class = vm.get_class(&fieldref.class_name);

                let value = frame.op_stack.pop().unwrap();
                let object_ref = frame.op_stack.pop().unwrap().1 as *mut u8;

                VirtualMachine::put_field(object_ref, &class, &fieldref.name, value.1);
            },
            0xb7 => { //invokespecial https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-6.html#jvms-6.5.invokespecial
                let index = frame.code.read_u16::<BigEndian>().unwrap();

                let method_ref = frame.class.constant_pool.resolve_ref_info(index as usize);
            },
            0xb8 => { //invokestatic
                let index = frame.code.read_u16::<BigEndian>().unwrap();

                let method_ref = frame.class.constant_pool.resolve_ref_info(index as usize);

                let (was_just_loaded, class ) = vm.load_and_link_class(method_ref.class_name.as_str()); //Load the class if it isn't already loaded

                let md = MethodDescriptor::parse(method_ref.descriptor.as_str());
                let method = class.get_method(method_ref.name.as_str(), method_ref.descriptor.as_str());

                if AccessFlags::is_native(&method.access_flags) { //Is native
                    let mut argument_stack: Vec<Operand> = Vec::new();
                    for _ in md.parameters.iter() {
                        argument_stack.push({
                            let operand = frame.op_stack.pop();
                            if operand.is_some() {
                                operand.unwrap()
                            } else {
                                panic!(
                                    format!("Attempted to pop operand stack, is empty. Bytecode {}\n Method name \"{}\"", opcode_pos, method.name)
                                )
                            }
                        });
                    }
                    vm.invoke_native(&method.name, &class, argument_stack);
                } else if was_just_loaded {
                    let mut new_frame = RuntimeThread::create_frame(
                        method.name.as_str(),
                        method.descriptor.as_str(),
                        &class
                    );

                    for i in 0..md.parameters.len() {
                        new_frame.local_vars.insert(
                            i as u16,
                            Operand::as_type(frame.op_stack.pop().unwrap())
                        );
                    }

                    pending_frames = Option::Some(vec![ //First is at top of stack
                        RuntimeThread::create_frame(
                            "<clinit>",
                            "()V",
                            &class
                        ),
                        new_frame
                    ]);
                } else {
                    pending_frames = Option::Some(vec![
                        {
                            let mut new_frame = RuntimeThread::create_frame(
                                method.name.as_str(),
                                method.descriptor.as_str(),
                                &class
                            );

                            for i in 0..md.parameters.len() {
                                new_frame.local_vars.insert(
                                    i as u16,
                                    Operand::as_type(frame.op_stack.pop().unwrap())
                                );
                            }

                            new_frame
                        }
                    ]);
                }
            },
            ///new
            ///breakpoint
            0xbb => {
                let index = frame.code.read_u16::<BigEndian>().unwrap();
                let classpath = frame.class.constant_pool.resolve_class_info(index as usize);
                let (loaded, class) = vm.load_and_link_class(classpath);

                let ptr = VirtualMachine::allocate_class(&class);

                let mut frames: Vec<Frame> = vec![];

                let mut obj_frame = RuntimeThread::create_frame(
                    "<init>",
                    "()V",
                    &class
                );

                obj_frame.local_vars.insert(0, Type::Reference(Reference::Class(ptr)));

                frames.push(obj_frame);

                //Because of how pending_frames adds frames to the stack, you put it in the reverse order, so the top would end up being at the end of the vec.

                if loaded && class.field_map.contains_key("<clinit>") { //Just loaded, we need to run <clinit>
                    frames.push(
                        RuntimeThread::create_frame(
                            "<clinit>",
                            "()V",
                            &class
                        )
                    );
                }

                frame.op_stack.push(Operand(OperandType::ClassReference, ptr as usize));

                pending_frames = Option::Some(frames);
            },
            0xca => {
                // println!("Breakpoint!");
            }
            _ => {
                unimplemented!("\n\nOpcode: {}\nClass: {}\nMethod: {}\nIndex: {}\n\n", opcode, frame.class.this_class, frame.method_name, frame.code.position()-1);
            }
        }

        if pending_frames.is_some() {
            let mut frames = pending_frames.unwrap();
            for _ in 0..frames.len() {
                self.stack.push(frames.pop().unwrap());
            }
        }
    }

    pub fn debug_op_stack(&self) {
        println!("\nDebug operands\n");
        for val in self.stack.last().unwrap().op_stack.iter() {
            println!("Type: {:?}      Value: {}", val.0, val.1);
        }
    }

    // pub fn get_local_var(&self, index: u16) -> &Type {
    //     self.stack.
    // }

    pub fn get_stack_count(&self) -> usize {
        self.stack.len()
    }
}
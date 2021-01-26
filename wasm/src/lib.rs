#![feature(try_trait)]

mod ir;

use jvm;
use jvm::vm::class::{Class, Method, MethodDescriptor, FieldDescriptor, BaseType, MethodReturnType, AccessFlags};
use std::rc::Rc;
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use jvm::vm::class::attribute::AttributeItem::Signature;
use std::error::Error;
use jvm::vm::class::attribute::{AttributeItem, Code};
use std::io::{Cursor, Seek, SeekFrom};
use jvm::vm::class::constant::Constant;
use std::collections::HashMap;
use byteorder::{ReadBytesExt, BigEndian, LittleEndian, ByteOrder};
use jvm::vm::vm::VirtualMachine;
use jvm::vm::linker::loader::ClassLoader;
use std::cmp::max;
use walrus::{Module, ModuleConfig, LocalFunction, ValType, FunctionBuilder, FunctionId, ImportId, LocalId, ModuleLocals, MemoryId, FunctionKind, ExportItem, ImportKind, Memory, InstrSeqBuilder};
use walrus::ir::*;
use walrus::ir::{BinaryOp, StoreKind, MemArg, LoadKind, Instr, Value, InstrSeqId, InstrSeqType};
use std::io;
use std::option::NoneError;
use jvm::vm::vm::bytecode::Bytecode;
use crate::ir::{ControlFlow, Intermediate2, IndexedBytecode};
use std::cell::{RefCell, RefMut};
use std::borrow::{Borrow, BorrowMut};
use std::ops::DerefMut;

fn format_method_name(class: &Class, method: &Method) -> String {
    format!("{}!{}!{}", class.this_class, &method.name, &method.descriptor)
}

fn base_type_as_value_type(bt: &BaseType) -> ValType {
    match bt {
        BaseType::Byte => ValType::I32,
        BaseType::Char => ValType::I32,
        BaseType::Double => ValType::F64,
        BaseType::Float => ValType::F32,
        BaseType::Int => ValType::I32,
        BaseType::Long => ValType::I64,
        BaseType::Reference => ValType::I64,
        BaseType::Bool => ValType::I32,
        BaseType::Short => ValType::I32
    }
}

fn field_descriptor_as_value_type(fd: &FieldDescriptor) -> ValType {
    match fd {
        FieldDescriptor::BaseType(bt) => base_type_as_value_type(bt),
        FieldDescriptor::ObjectType(_) => ValType::I32,
        FieldDescriptor::ArrayType(_) => ValType::I32
    }
}

pub enum ResourceType {
    U32(u32),
    I32(i32),
    UTF8(String),
    ObjectInstance {
        classpath_offset: u32,
        fields: Vec<u8>,
        fields_length: i32
    }, //Manually created data in the memory layout, like a pre-allocated class
    Null
}

impl ResourceType {
    pub fn size_of(&self) -> u32 {
        match &self {
            ResourceType::U32(_) => 4,
            ResourceType::I32(_) => 4,
            ResourceType::UTF8(str) => (str.len() * 2) as u32 + 4,
            ResourceType::ObjectInstance { classpath_offset, fields_length, .. } => 4 + *fields_length as u32,
            ResourceType::Null => 1
        }
    }

    pub fn as_vec(&self) -> Vec<u8> {
        match self {
            ResourceType::U32(u) => {
                let mut buf = [0u8; 4];
                LittleEndian::write_i32(&mut buf[..], *u as i32);
                buf.into()
            },
            ResourceType::I32(i) => {
                let mut buf = [0u8; 4];
                LittleEndian::write_i32(&mut buf[..], *i);
                buf.into()
            },
            ResourceType::UTF8(string) => {
                let mut vec = vec![0,0,0,0];
                LittleEndian::write_u32(&mut vec[..], string.len() as u32);

                for char in string.as_bytes() {
                    vec.push(*char);
                    vec.push(0);
                }

                vec
            }
            ResourceType::ObjectInstance { classpath_offset, fields, fields_length } => {
                let mut vec = vec![0,0,0,0];
                LittleEndian::write_i32(&mut vec[..], *classpath_offset as i32);
                vec.extend_from_slice(&fields[..*fields_length as usize]);

                vec
            }
            ResourceType::Null => vec![ 0u8 ]
        }
    }
}

pub struct Resource {
    pub res_type: ResourceType,
    pub offset: u32
}

/*

Layout of allocated classes in memory

+---------------+----------------------------+------------------------------------+
| <header>: u32 | <[u32]>: this-class fields | [u32]: recursive superclass fields |
+---------------+----------------------------+------------------------------------+

header: an offset pointer which points to the class info, located somewhere in the linear memory
`
 */

struct MemoryLayout {
    pub resources: Vec<Resource>,
    pub strings: HashMap<String, usize>,
    pub constant_resource_map: HashMap<u16, usize>,
    pub null: u32,
    pub heap_size: u32,
    pub memory_id: MemoryId
}

impl MemoryLayout {
    pub fn new(memory_id: MemoryId) -> Self {
        Self {
            resources: vec![
                Resource { res_type: ResourceType::U32(1 + 16 + 4), offset: 0 }, //Heap size, including the heap size u32
                Resource { res_type: ResourceType::Null, offset: 4 }, //Only needs to be 1 byte long
                Resource { res_type: ResourceType::UTF8(String::from("java/lang/String")), offset: 5 }
            ],
            strings: HashMap::new(),
            constant_resource_map: HashMap::new(),
            null: 1,
            heap_size: 0,
            memory_id
        }
    }

    pub fn append_resource(&mut self, resource_type: ResourceType) -> usize {
        let last = self.resources.last().unwrap();
        let offset = last.offset + last.res_type.size_of();

        let resource_size = resource_type.size_of();

        self.resources.push(Resource {
            res_type: resource_type,
            offset
        });

        let resource: &mut Resource = self.resources.get_mut(self.heap_size as usize).unwrap();

        match &mut resource.res_type {
            ResourceType::U32(size) => {
                *size += resource_size;
            },
            _ => {}
        }

        self.resources.len() - 1
    }

    pub fn allocate_string(&mut self, string: String, string_class: &Class) -> usize {
        let size = string_class.full_heap_size + 4; //Following the memory layout

        let resource_index = self.append_resource(ResourceType::UTF8(string));
        let utf8 = self.resources.get(
            resource_index
        ).unwrap().offset;

        let mut fields = vec![0, 0, 0, 0];

        LittleEndian::write_i32(&mut fields[..], utf8 as i32);

        self.append_resource(ResourceType::ObjectInstance {
            classpath_offset: 5,
            fields,
            fields_length: string_class.full_heap_size as i32
        })
    }

    pub fn as_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            self.resources.last().unwrap().offset as usize
        );

        let mut last_index = 0;

        for res in self.resources.iter() {
            for _ in last_index..res.offset {
                out.push(0u8); //pad
            }

            out.append(&mut res.res_type.as_vec());
            last_index = res.offset+res.res_type.size_of();
        }

        out
    }
}

struct StackHelper<'locals> {
    vars: HashMap<ValType, Vec<LocalId>>,
    types: Vec<ValType>,
    locals: &'locals mut ModuleLocals
}

impl<'locals> StackHelper<'locals> {
    pub fn new(module: &'locals mut ModuleLocals) -> Self {
        Self {
            vars: HashMap::new(),
            types: Vec::new(),
            locals: module
        }
    }

    pub fn get(&mut self, t: ValType, offset: usize) -> LocalId {
        if !self.vars.contains_key(&t) {
            self.vars.insert(t, Vec::new());
        }

        let vec = self.vars.get_mut(&t).unwrap();

        if offset+1 > vec.len() {
            let diff = offset + 1 - vec.len();

            for _ in 0..diff {
                vec.push(
                    self.locals.add(t)
                );

                self.types.push(t)
            }
        }

        vec.get(offset).unwrap().clone()
    }
}

#[derive(Debug)]
pub enum MethodFunctionType {
    Normal(FunctionId, Vec<LocalId>),
    NativeImport(FunctionId, ImportId)
}

impl MethodFunctionType {

    fn unwrap_normal(&self) -> (FunctionId, &Vec<LocalId>) {
        match self {
            MethodFunctionType::Normal(func, vec) => (*func, vec),
            MethodFunctionType::NativeImport(_, _) => panic!("Tried to unwrap normal function, was an import!")
        }
    }

}

pub struct SerializedMemory {
    pub data: Vec<u8>,
    pub strings: HashMap<String, usize>
}

macro_rules! xload_n {
    ($n:expr, $md:ident, $variables:ident) => {
        $variables.get(&$n).ok_or(CompilationError::UnknownLocal($n))?.clone()
    }
}

pub struct WasmEmitter<'classloader> {
    pub(crate) module: RefCell<Module>,
    pub(crate) memory: RefCell<MemoryLayout>,
    pub(crate) method_function_map: HashMap<String, MethodFunctionType>,
    pub(crate) class_loader: &'classloader ClassLoader,

    pub class_resource_map: HashMap<String, u32>, //String being the classpath, and the u32 being a location in the resources
    pub main_export: String
}

#[derive(Debug)]
pub enum CompilationError {
    UnknownLocal(usize),
    InvalidBytecode,
    EOF,
    NoneError,
    MethodNotFound,
    InvalidIntermediate
}

impl From<io::Error> for CompilationError {
    fn from(_: io::Error) -> Self {
        Self::EOF
    }
}

impl From<NoneError> for CompilationError {
    fn from(_: NoneError) -> Self {
        Self::NoneError
    }
}

impl<'classloader> WasmEmitter<'classloader> {
    pub fn new(class_loader: &'classloader ClassLoader, main_export: &str) -> Self {
        let mut module = Module::with_config(ModuleConfig::new());

        let memory_id = module.memories.add_local(
            false,
            10,
            Option::None
        );

        module.exports.add(
            "heap",
            ExportItem::Memory(memory_id)
        );

        let mut emitter = WasmEmitter {
            module: RefCell::new(module),
            memory: RefCell::new(MemoryLayout::new(memory_id)),
            method_function_map: HashMap::new(),
            class_loader,
            class_resource_map: HashMap::new(),
            main_export: String::from(main_export)
        };

        {
            let mut module = emitter.module.borrow_mut();

            {
                let requested_size = module.locals.add(ValType::I32);

                let mut heap_allocate = FunctionBuilder::new(&mut module.types, &[ValType::I32], &[ValType::I32]);

                let local = module.locals.add(ValType::I32);

                heap_allocate.name(String::from("allocate_to_heap")).func_body()
                    .i32_const(emitter.memory.borrow().heap_size as i32)
                    .load(
                        emitter.memory.borrow().memory_id,
                        LoadKind::I32 {
                            atomic: false
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    )
                    .local_get(requested_size)
                    .binop(BinaryOp::I32Add) //Get the new heap size
                    .local_set(local) //Store it
                    .local_get(local) //Get it again
                    .i32_const(emitter.memory.borrow().heap_size as i32) //offset to where the heap size is located
                    .store(
                        emitter.memory.borrow().memory_id,
                        StoreKind::I32 {
                            atomic: false
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    ) //Update the heap size
                    .local_get(local); //Get the new heap size

                let id = heap_allocate.finish(vec![requested_size], &mut module.funcs);

                emitter.method_function_map.insert(String::from("allocate_to_heap"), MethodFunctionType::Normal(id, vec![requested_size]));
            }

            //TODO: make these do stuff again

            {
                let class = module.locals.add(ValType::I32);

                let mut heap_allocate = FunctionBuilder::new(&mut module.types, &[ValType::I32], &[ValType::I32]);

                heap_allocate.name(String::from("heap_allocate")).func_body().local_get(class);

                let id = heap_allocate.finish(vec![class], &mut module.funcs);

                emitter.method_function_map.insert(String::from("allocate_class"), MethodFunctionType::Normal(id, vec![class]));
            }
        }

        emitter
    }

    pub fn build(mut self) -> (Vec<u8>, SerializedMemory) {
        let data = self.memory.borrow().as_vec();
        let strings = self.memory.into_inner().strings;

        (self.module.borrow_mut().emit_wasm(), SerializedMemory {
            data,
            strings
        })
    }

    pub fn process_classes(&mut self) {
        for (classpath, class) in self.class_loader.class_map.iter() {
            let clazz = class.unwrap();
            self.generate_stubs(clazz.as_ref());
        }

        for (classpath, class) in self.class_loader.class_map.iter() {
            let clazz = class.unwrap();

            self.process_class(clazz.as_ref());
        }
    }

    pub fn generate_stubs(&mut self, class: &Class) {
        let mut module = self.module.borrow_mut();

        for (method_name, overloaded) in class.method_map.iter() {
            for (descriptor, method) in overloaded.iter() {
                let method_descriptor = MethodDescriptor::parse(&method.descriptor);
                let formatted = format_method_name(class, method);

                let mut params_vec: Vec<ValType> = method_descriptor.parameters.iter().map(|p| field_descriptor_as_value_type(p)).collect();

                if method.access_flags & 0x8 == 0 { //Not static
                    params_vec.insert(
                        0, ValType::I32
                    ); //the first local variabe in an object method is the "this" reference
                }

                let return_vec = match &method_descriptor.return_type {
                    MethodReturnType::Void => vec![],
                    MethodReturnType::FieldDescriptor(f) => vec![field_descriptor_as_value_type(f)]
                };

                if AccessFlags::is_native(method.access_flags) {
                    let native_type = module.types.add(
                        &params_vec[..],
                        &return_vec[..]
                    );

                    let import = module.add_import_func(
                        "jvm_native",
                        &formatted,
                        native_type
                    );

                    self.method_function_map.insert(String::from(&formatted), MethodFunctionType::NativeImport(import.0, import.1));
                } else {
                    let mut builder = FunctionBuilder::new(
                        &mut module.types,
                        &params_vec[..],
                        &return_vec[..]
                    );

                    builder.name(
                        formatted
                    );

                    let param_locals: Vec<LocalId> = params_vec.iter().map(|x| module.locals.add(*x)).collect();

                    let id = builder.finish(
                        param_locals.clone(),
                        &mut module.funcs
                    );

                    self.method_function_map.insert(
                        format_method_name(class, method),
                        MethodFunctionType::Normal(id, param_locals)
                    );

                    if method.name == "main" {
                        module.exports.add(
                            "main",
                            ExportItem::Function(id)
                        );
                    }
                }
            }
        }
    }

    pub fn process_class(&mut self, class: &Class) {
        for (method_name, overloaded) in class.method_map.iter() {
            for (descriptor, method) in overloaded.iter() {
                self.process_method(class, method.as_ref());
            }
        }
    }

    pub fn process_method(&mut self, class: &Class, method: &Method) -> Option<()> {
        let method_descriptor = MethodDescriptor::parse(&method.descriptor);

        let formatted = format_method_name(class, method);

        if method.attribute_map.contains_key("Code") {
            if let AttributeItem::Code(code) = &method.attribute_map.get("Code")?.info {

                let mut bytecode_locals = Self::analyze_locals(code, class);

                let pre_params = self.method_function_map.get(&formatted)?.unwrap_normal().1;

                let mut java_locals = HashMap::new();

                let mut min_local_index = 0;
                let mut max_local_index = 0;

                for local in bytecode_locals.iter() {
                    if *local.0 < min_local_index { min_local_index = *local.0; }
                    if *local.0 > max_local_index { max_local_index = *local.0; }
                }

                let params_length = pre_params.len();

                let max_parameter_index = max(params_length as isize - 1, 0);

                let local_variables_not_params = max(max_local_index as isize - max_parameter_index, 0) as usize;

                for index in 0..params_length {
                    java_locals.insert(
                        index,
                        pre_params.get(index).expect(&format!("{} {} {} {:?}", method.name, index, class.this_class, pre_params)).clone()
                    );

                }
                let mut module = self.module.borrow_mut();

                for index in 0..local_variables_not_params {
                    let offset = params_length+index;

                    java_locals.insert(offset, module.locals.add(bytecode_locals.get(&offset).expect(
                        &format!("{} {} {} {:?}", method.name, index, class.this_class, bytecode_locals)
                    ).clone()));
                }

                let intermediate = self.bytecode_intermediate(
                    code, method, class
                );

                println!("Intermediate {:?}", intermediate);

                let func_id = self.method_function_map.get(&formatted)?.unwrap_normal().0;

                let mut builder = match &mut module.funcs.get_mut(func_id).kind {
                    FunctionKind::Import(_) => unreachable!(),
                    FunctionKind::Local(local) => local.builder_mut(),
                    FunctionKind::Uninitialized(_) => unreachable!(),
                };

                let stubs = Self::generate_block_stubs(
                    &mut builder.func_body(),
                    &intermediate,
                    true
                ).unwrap();

                let mut index_map = HashMap::new();

                stubs.iter().for_each(|(bytecode_index, i2, block_id)| {
                    index_map.insert(*bytecode_index, *block_id);
                });

                stubs.iter().for_each(|(bytecode_index, i2, block_id)| {
                    let instructions = match i2 {
                        Intermediate2::Intermediate1(_) => unreachable!(),
                        Intermediate2::Instructions(v) => v,
                        Intermediate2::Block(_) => unreachable!()
                    };

                    let instr;

                    let locals = &mut module.locals;
                    instr = self.compile_bytecode(locals, instructions, &index_map, method, class, &java_locals).unwrap();

                    let mut builder = match &mut module.funcs.get_mut(func_id).kind {
                        FunctionKind::Import(_) => unreachable!(),
                        FunctionKind::Local(local) => local.builder_mut(),
                        FunctionKind::Uninitialized(_) => unreachable!(),
                    };

                    instr.iter().for_each(|i| {
                        builder.instr_seq(*block_id).instr(i.clone());
                    });
                });

                // match self.compile_bytecode(code, method, class, java_locals) {
                //     Err(c) => panic!(format!("Failed to compile code for {}\n{:?}", formatted, c)),
                //     Ok(_) => {}
                // };
            }
        }

        Option::Some(())
    }

    ///Returns every local variable that is used and it's type
    fn analyze_locals(code: &Code, class: &Class) -> HashMap<usize, ValType> {
        let mut bytes = Cursor::new(code.code.clone());
        let len = code.code.len();

        let mut jvm_locals = HashMap::new();

        while bytes.position() < len as u64 {
            let opcode = bytes.read_u8().unwrap();

            match opcode {
                0x15 => { //iload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::I32);
                },
                0x16 => { //lload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::I64);
                },
                0x17 => { //fload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::F32);
                },
                0x18 => { //dload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::F64);
                },
                0x19 => { //aload
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::I32);
                },
                0x1a..=0x1d => { //iload_<n>
                    let index = (opcode as u32) - 0x1a;

                    jvm_locals.insert(index as usize, ValType::I32);
                },
                0x1e..=0x21 => { //lload_<n>
                    let index = (opcode as u32) - 0x1e;

                    jvm_locals.insert(index as usize, ValType::I64);
                },
                0x22..=0x25 => { //fload_<n>
                    let index = (opcode as u32) - 0x22;

                    jvm_locals.insert(index as usize, ValType::F32);
                },
                0x26..=0x29 => { //dload_<n>
                    let index = (opcode as u32) - 0x26;

                    jvm_locals.insert(index as usize, ValType::F64);
                },
                0x2a..=0x2d => { //aload_<n>
                    let index = (opcode as u32) - 0x2a;

                    jvm_locals.insert(index as usize, ValType::I32);
                },
                0x3a => { //astore
                    let index = bytes.read_u8().unwrap() as u32;

                    jvm_locals.insert(index as usize, ValType::I32);
                },
                0x3b..=0x3e => { //istore_<n>
                    jvm_locals.insert(opcode as usize - 0x3b, ValType::I32);
                }
                0x4b..=0x4e => { //astore_<n>
                    jvm_locals.insert(opcode as usize - 0x4b, ValType::I32);
                },
                _ => {
                    bytes.seek(
                    SeekFrom::Current(Bytecode::size_of(
                        &Bytecode::from_bytes(
                            bytes.position() as usize - 1,
                            &bytes.get_ref()[bytes.position() as usize-1..bytes.get_ref().len()]
                        ).unwrap()
                    ) as i64)
                ); }
            }
        }

        jvm_locals
    }

    fn bytecode_intermediate(&self, code: &Code, method: &Method, class: &Class) -> Intermediate2 {
        ControlFlow::convert(&code.code[..]).unwrap()
    }

    fn generate_block_stubs<'intermediate>(builder: &mut InstrSeqBuilder, intermediates: &'intermediate Intermediate2, first: bool) -> Result<Vec<(usize, &'intermediate Intermediate2, InstrSeqId)>, CompilationError> {
        let mut vec: Vec<(usize, &'intermediate Intermediate2, InstrSeqId)> = Vec::new();

        if first {
            if let Intermediate2::Block(v) = intermediates {
                v.iter().for_each(|i| {
                    vec.extend(
                        Self::generate_block_stubs(builder, i, false).unwrap()
                    );
                });

                return Result::Ok(vec);
            }
        }

        match intermediates {
            Intermediate2::Intermediate1(_) => return Result::Err(CompilationError::InvalidIntermediate),
            Intermediate2::Instructions(instr) => {
                let first = instr.first();
                if first.is_some() {
                    vec.push(
                        (first.unwrap().1, intermediates, builder.id())
                    );
                } else {
                    panic!("shouldn't happen");
                }
            }
            Intermediate2::Block(i2) => {
                builder.block(InstrSeqType::Simple(Option::None), |builder| {
                    i2.iter().for_each(|e| {
                        vec.extend(
                            Self::generate_block_stubs(
                                builder, e, false
                            ).unwrap()
                        )
                    });
                });
            }
        }

        Result::Ok(vec)
    }

    fn compile_bytecode(&self, locals: &mut ModuleLocals, code: &Vec<IndexedBytecode>, block_map: &HashMap<usize, InstrSeqId>, method: &Method, class: &Class, java_locals: &HashMap<usize, LocalId>) -> Result<Vec<Instr>, CompilationError> {
        let formatted = format_method_name(class, method);
        let method_descriptor = MethodDescriptor::parse(&method.descriptor);

        let mut locals_max = 0; //The max index used to the local map in the bytecode

        let mut helper = StackHelper::new(locals);

        java_locals.iter().for_each(|i| {
            if *i.0 > locals_max { locals_max = *i.0; }
        });

        let mut builder = ReallyLazyBuilderReplacement {
            instrs: vec![]
        };

        for ib in code.iter() {

            let bytecode = &ib.0;
            let bytecode_index = *&ib.1;

            match bytecode {
                Bytecode::Aconst_null => { //aconst_null
                    builder.i32_const(self.memory.borrow().null as i32);
                },
                Bytecode::Iconst_n_m1(i) => {
                    builder.i32_const(*i as i32);
                },
                Bytecode::Bipush(b) => {
                    builder.i32_const(*b as i32);
                },
                Bytecode::Ldc(index) => { //ldc
                    match class.constant_pool.get(*index as usize).unwrap() {
                        Constant::Integer(int) => { builder.i32_const(*int); },
                        Constant::Float(float) => { builder.f32_const(*float); },
                        Constant::Long(long) => { builder.i64_const(*long); },
                        Constant::Double(double) => { builder.f64_const(*double as f64); },
                        Constant::String(string) => {
                            let utf8 = class.constant_pool.resolve_utf8(*string).unwrap();

                            let resource_index = if !self.memory.borrow().strings.contains_key(utf8) {
                                self.memory.borrow_mut().allocate_string(String::from(utf8), self.class_loader.get_class("java/lang/String").unwrap().as_ref())
                            } else {
                                *self.memory.borrow().strings.get(utf8).unwrap()
                            };

                            builder.i32_const(self.memory.borrow().resources.get(resource_index).unwrap().offset as i32);
                        }
                        _ => {}
                    };
                },
                Bytecode::Sipush(short) => { //sipush
                    builder.i32_const(*short as i32);
                },
                Bytecode::Iload(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Lload(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Fload(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Dload(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Aload(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Iload_n(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Lload_n(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Fload_n(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Aload_n(index) => {
                    builder.local_get(xload_n!(*index as usize, method_descriptor, java_locals));
                },
                Bytecode::Aaload => { //aaload
                    builder.binop(BinaryOp::I32Add);
                    builder.load(
                        self.memory.borrow().memory_id,
                        LoadKind::I32 {
                            atomic: false
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                Bytecode::Baload => { //baload
                    builder.i32_const(1);
                    builder.binop(BinaryOp::I32Add);

                    builder.binop(BinaryOp::I32Add); //Increment by one

                    //Arrayref is now at the top of the stack
                    builder.binop(BinaryOp::I32Add);
                    //The proper offset to the element is now at the top
                    builder.store(
                        self.memory.borrow().memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    ); //Load the byte/bool
                },
                Bytecode::Astore(local) => { //astore
                    builder.local_set(java_locals.get(&(*local as usize)).ok_or(CompilationError::UnknownLocal(*local as usize))?.clone());
                },
                Bytecode::Istore_n(local) => { //istore_<n>
                    builder.local_set(java_locals.get(&(*local as usize)).ok_or(CompilationError::UnknownLocal(*local as usize))?.clone());
                },
                Bytecode::Astore_n(local) => { //astore_<n>
                    builder.local_set(java_locals.get(&(*local as usize)).ok_or(CompilationError::UnknownLocal(*local as usize))?.clone());
                },
                Bytecode::Aastore => { //aastore
                    builder.local_set(helper.get(ValType::I32, 0));
                    builder.binop(BinaryOp::I32Add);
                    builder.local_get(helper.get(ValType::I32, 0));
                    builder.store(
                        self.memory.borrow().memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                Bytecode::Bastore => { //bastore
                    builder.local_set(helper.get(ValType::I32, 0));

                    //arrayref is on top

                    //TODO: null check

                    builder.i32_const(1); //Increment by one because the length is first
                    builder.binop(BinaryOp::I32Add); //index + 1
                    builder.binop(BinaryOp::I32Add); //(index + 1) + arrayref

                    builder.local_get(helper.get(ValType::I32, 0)); //Un-stash the value
                    builder.store(
                        self.memory.borrow().memory_id,
                        StoreKind::I32_8 {
                            atomic: false
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                Bytecode::Castore => { //castore
                    //arrayref, index, value

                    builder.local_set(helper.get(ValType::I32, 0)) //stash the value
                        .i32_const(2)
                        .binop(BinaryOp::I32Mul) //Multiply by two as a char is two bytes
                        .i32_const(4)
                        .binop(BinaryOp::I32Add) //(index * 2) + arrayref + 4
                        .local_get(helper.get(ValType::I32, 0))
                        .store(
                            self.memory.borrow().memory_id,
                            StoreKind::I32_16 {
                                atomic: false
                            },
                            MemArg {
                                align: 1,
                                offset: 0
                            }
                        );
                },
                Bytecode::If_icmpge(branch) => { //if_icmpge
                    builder.binop(BinaryOp::I32GeS);

                    let index = (bytecode_index as isize) + (*branch as isize);

                    builder.br_if(
                        *block_map.get(&(index as usize)).unwrap()
                    );
                },
                Bytecode::Ireturn | Bytecode::Areturn | Bytecode::Return => {
                    builder.return_();
                },
                Bytecode::Putfield(index) => {
                    if let Constant::FieldRef(class_index, nat_index) = class.constant_pool.get(*index as usize).unwrap() {
                        let clazz = class.constant_pool.resolve_class_info(*class_index).unwrap();
                        let (name, descriptor) = class.constant_pool.resolve_name_and_type(*nat_index).unwrap();

                        let field_offset = class.field_map.get(name).unwrap().offset;
                        let fd = FieldDescriptor::parse(descriptor);

                        builder.local_set(
                            helper.get(ValType::I32, 0)
                        ); //stash

                        builder.i32_const(field_offset as i32);
                        builder.i32_const(1);

                        builder.binop(BinaryOp::I32Add);
                        builder.binop(BinaryOp::I32Add);

                        builder.local_get(
                            helper.get(ValType::I32, 0)
                        );
                    } else { panic!("Constant did not resolve to FieldRef!"); }
                },
                Bytecode::Invokespecial(index) => { //TODO: invokespecial

                    let method_ref = class.constant_pool.resolve_ref_info(*index as usize).unwrap();

                    let method_descriptor = MethodDescriptor::parse(&method_ref.descriptor);

                    // println!("{}", method_d);

                    let method_class = self.class_loader.get_class(&method_ref.class_name).unwrap();
                    let resolved_method = method_class.get_method(&method_ref.name, &method_ref.descriptor);
                    let to_invoke;

                    if {
                        (method_class.access_flags & 0x20 == 0x20)
                            && self.class_loader.recurse_is_superclass(class, &method_class.this_class)
                            && resolved_method.name != "<init>"
                    } {
                        to_invoke = self.class_loader.recurse_resolve_supermethod_special(
                            self.class_loader.get_class(&class.super_class).unwrap(),
                            &resolved_method.name,
                            &resolved_method.descriptor,
                        ).unwrap();
                    } else {
                        to_invoke = (method_class, resolved_method);
                    }

                    let function_id = match self.method_function_map.get(&format_method_name(
                        to_invoke.0.as_ref(),
                        to_invoke.1.as_ref()
                    )).unwrap() {
                        MethodFunctionType::Normal(id, _) => id,
                        MethodFunctionType::NativeImport(id, _) => id
                    };

                    builder.call(*function_id);
                },
                Bytecode::Invokestatic(index) => { //TODO: invokestatic
                    let method_ref = class.constant_pool.resolve_ref_info(*index as usize).unwrap();
                    let clazz = self.class_loader.get_class(&method_ref.class_name).unwrap();

                    let method = clazz.get_method(&method_ref.name, &method_ref.descriptor);

                    if AccessFlags::is_native(method.access_flags) {
                        builder.call(
                            match self.method_function_map.get(&format_method_name(clazz.as_ref(), method.as_ref())).unwrap() {
                                MethodFunctionType::Normal(_, _) => unreachable!(),
                                MethodFunctionType::NativeImport(func, _) => *func
                            }
                        );
                    }
                },
                Bytecode::Anewarray(index) => { //anewarray
                    builder.i32_const(1); //Size of the array takes up an I32
                    builder.binop(BinaryOp::I32Add);
                    builder.call(
                        match self.method_function_map.get("allocate_to_heap").unwrap() {
                            MethodFunctionType::Normal(funcid, _) => funcid.clone(),
                            MethodFunctionType::NativeImport(_, _) => unreachable!()
                        }
                    );
                },
                Bytecode::New(index) => {
                    let classpath = class.constant_pool.resolve_class_info(*index)?;
                    let clazz = self.class_loader.get_class(classpath)?;
                    let heap_classpath_offset = *self.class_resource_map.get(classpath)?;

                    builder
                        .i32_const(clazz.full_heap_size as i32 + 4) //Size of the class, plus the offset to the classpath
                        .call(self.method_function_map.get("allocate_to_heap")?.unwrap_normal().0)
                        .local_set(helper.get(ValType::I32, 0)) //Store it
                        .local_get(helper.get(ValType::I32, 0))
                        .i32_const(heap_classpath_offset as i32) //offset to the classpath
                        /*
                        The stack now looks like:

                        [pointer to class reference start]
                        [pointer to where the classpath string is stored]
                         */
                        .store(
                            self.memory.borrow().memory_id,
                            StoreKind::I32 {
                                atomic: false
                            },
                            MemArg {
                                align: 1,
                                offset: 0
                            }
                        )
                        .local_get(helper.get(ValType::I32, 0)); //Get the pointer (offset) to the reference again

                },
                Bytecode::Ifeq(c) => {

                },
                Bytecode::Newarray(atype) => { //newarray
                    let size = match *atype {
                        4 => 1,
                        5 => 2,
                        6 => 4,
                        7 => 8,
                        8 => 1,
                        9 => 2,
                        10 => 4,
                        11 => 8,
                        _ => return Result::Err(
                            CompilationError::InvalidBytecode
                        )
                    };

                    builder.i32_const(size) //The size of the data type
                        .binop(BinaryOp::I32Mul) //Multiply it with the count which is on the stack
                        .call(self.method_function_map.get("allocate_to_heap")?.unwrap_normal().0); //Allocate
                },
                Bytecode::Arraylength => { //arraylength
                    builder.load(
                        self.memory.borrow().memory_id,
                        LoadKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    ); //the pointer to the array will be the length itself
                },
                _ => unimplemented!("Unimplemented opcode {:?}\nClass: {}\nMethod: {}", bytecode, class.this_class, method.name)
            }
        }

        Result::Ok(builder.instrs)
    }
}

struct ReallyLazyBuilderReplacement {
    pub(crate) instrs: Vec<Instr>
}

impl ReallyLazyBuilderReplacement {
    pub fn i32_const(&mut self, i: i32) -> &mut Self {
        self.instrs.push(
            Instr::Const(
                Const {
                    value: Value::I32(i)
                }
            )
        );
        self
    }

    pub fn i64_const(&mut self, i: i64) -> &mut Self {
        self.instrs.push(
            Instr::Const(
                Const {
                    value: Value::I64(i)
                }
            )
        );
        self
    }

    pub fn f32_const(&mut self, i: f32) -> &mut Self {
        self.instrs.push(
            Instr::Const(
                Const {
                    value: Value::F32(i)
                }
            )
        );
        self
    }

    pub fn f64_const(&mut self, i: f64) -> &mut Self {
        self.instrs.push(
            Instr::Const(
                Const {
                    value: Value::F64(i)
                }
            )
        );
        self
    }

    pub fn binop(&mut self, op: BinaryOp) -> &mut Self {
        self.instrs.push(
            Instr::Binop(
                Binop {
                    op
                }
            )
        );
        self
    }

    pub fn load(&mut self, mem: MemoryId, kind: LoadKind, arg: MemArg) -> &mut Self {
        self.instrs.push(
            Instr::Load(
                Load {
                    memory: mem,
                    kind,
                    arg
                }
            )
        );
        self
    }

    pub fn store(&mut self, mem: MemoryId, kind: StoreKind, arg: MemArg) -> &mut Self {
        self.instrs.push(
            Instr::Store(
                Store {
                    memory: mem,
                    kind,
                    arg
                }
            )
        );
        self
    }

    pub fn local_get(&mut self, local: LocalId) -> &mut Self {
        self.instrs.push(
            Instr::LocalGet(
                LocalGet {
                    local
                }
            )
        );
        self
    }

    pub fn local_set(&mut self, local: LocalId) -> &mut Self {
        self.instrs.push(
            Instr::LocalSet(
                LocalSet {
                    local
                }
            )
        );
        self
    }

    pub fn call(&mut self, func: FunctionId) -> &mut Self {
        self.instrs.push(
            Instr::Call(
                Call {
                    func
                }
            )
        );
        self
    }
    
    pub fn return_(&mut self) -> &mut Self {
        self.instrs.push(
            Instr::Return(
                Return {

                }
            )
        );
        self
    }

    pub fn br_if(&mut self, id: InstrSeqId) -> &mut Self {
        self.instrs.push(
            Instr::BrIf(
                BrIf {
                    block: id
                }
            )
        );
        self
    }
}
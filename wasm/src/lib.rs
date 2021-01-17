use jvm;
use jvm::vm::class::{Class, Method, MethodDescriptor, FieldDescriptor, BaseType, MethodReturnType};
use std::rc::Rc;
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use jvm::vm::class::attribute::AttributeItem::Signature;
use std::error::Error;
use jvm::vm::class::attribute::{AttributeItem, Code};
use std::io::Cursor;
use jvm::vm::class::constant::Constant;
use std::collections::HashMap;
use byteorder::{ReadBytesExt, BigEndian};
use jvm::vm::vm::VirtualMachine;
use jvm::vm::linker::loader::ClassLoader;
use std::cmp::max;
use walrus::{Module, ModuleConfig, LocalFunction, ValType, FunctionBuilder, FunctionId, ImportId, LocalId, ModuleLocals, MemoryId};
use walrus::ir::Instr::Binop;
use walrus::ir::{BinaryOp, StoreKind, MemArg, LoadKind};

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
    String(String),
    Class {
        classpath_offset: u32,
        fields: Vec<i32>,
        fields_length: i32
    }, //Manually created data in the memory layout, like a pre-allocated class
    Null
}

impl ResourceType {
    pub fn size_of(&self) -> u32 {
        match &self {
            ResourceType::U32(_) => 1,
            ResourceType::I32(_) => 1,
            ResourceType::String(str) => str.len() as u32 + 1,
            ResourceType::Class { classpath_offset, fields_length, .. } => 1 + *fields_length as u32,
            ResourceType::Null => 1
        }
    }
}

impl Into<Vec<i32>> for ResourceType {

    fn into(self) -> Vec<i32> {
        match self {
            ResourceType::U32(u) => vec![ u as i32 ],
            ResourceType::I32(i) => vec![ i ],
            ResourceType::String(string) => {
                let mut out = Vec::with_capacity(string.len() + 1);

                out.push(string.len() as i32);

                for byte in string.into_bytes().iter() {
                    out.push(*byte as i32);
                }

                out
            }
            ResourceType::Class { classpath_offset, fields, fields_length } => {
                let mut out = Vec::new();
                out.push(classpath_offset as i32);
                out.extend_from_slice(&fields[..fields_length as usize]);
                out
            }
            ResourceType::Null => vec![ 0i32 ]
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

pub struct MemoryLayout {
    pub resources: Vec<Resource>,
    pub strings: HashMap<String, usize>,
    pub constant_resource_map: HashMap<u16, usize>,
    pub null: u32,
    pub heap_size_offset: u32,
    pub memory_id: MemoryId
}

impl MemoryLayout {
    pub fn new(memory_id: MemoryId) -> Self {
        Self {
            resources: vec![
                Resource { res_type: ResourceType::Null, offset: 0 },
                Resource { res_type: ResourceType::U32(0), offset: 1 },
                Resource { res_type: ResourceType::String(String::from("java/lang/String")), offset: 2 }
            ],
            strings: HashMap::new(),
            constant_resource_map: HashMap::new(),
            null: 0,
            heap_size_offset: 1,
            memory_id
        }
    }

    pub fn append_resource(&mut self, resource_type: ResourceType) -> usize {
        let last = self.resources.last().unwrap();
        let offset = last.offset + last.res_type.size_of();

        self.resources.push(Resource {
            res_type: resource_type,
            offset
        });

        self.resources.len() - 1
    }

    pub fn allocate_string(&mut self, string: String, string_class: &Class) -> usize {
        let size = string_class.full_heap_size + 1; //Following the memory layout

        let utf8 = self.append_resource(ResourceType::String(string));

        self.append_resource(ResourceType::Class {
            classpath_offset: 2,
            fields: vec![ utf8 as i32 ],
            fields_length: string_class.full_heap_size as i32
        })
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
    Normal(FunctionId),
    NativeImport(FunctionId, ImportId)
}

pub struct WasmEmitter<'classloader> {
    pub module: Module,
    pub memory: MemoryLayout,
    pub function_count: u32,
    pub method_function_map: HashMap<String, MethodFunctionType>,
    pub class_loader: &'classloader ClassLoader,

    pub class_resource_map: HashMap<String, u32>, //String being the classpath, and the u32 being a location in the resources
    pub main_export: String
}

impl<'classloader> WasmEmitter<'classloader> {
    pub fn new(class_loader: &'classloader ClassLoader, main_export: &str) -> Self {
        let mut module = Module::with_config(ModuleConfig::new());

        let memory_id = module.memories.add_local(
            true,
            4096,
            Option::None
        );

        let mut emitter = WasmEmitter {
            module,
            memory: MemoryLayout::new(memory_id),
            function_count: 0,
            method_function_map: HashMap::new(),
            class_loader,
            class_resource_map: HashMap::new(),
            main_export: String::from(main_export)
        };

        {
            let requested_size = emitter.module.locals.add(ValType::I32);

            let mut heap_allocate = FunctionBuilder::new(&mut emitter.module.types, &[ValType::I32], &[ValType::I32]);

            heap_allocate.name(String::from("allocate_to_heap")).func_body().local_get(requested_size);

            let id = heap_allocate.finish(vec![requested_size], &mut emitter.module.funcs);

            emitter.method_function_map.insert(String::from("allocate_to_heap"), MethodFunctionType::Normal(id));
        }

        //TODO: make these do stuff again

        {
            let class = emitter.module.locals.add(ValType::I32);

            let mut heap_allocate = FunctionBuilder::new(&mut emitter.module.types, &[ValType::I32], &[ValType::I32]);

            heap_allocate.name(String::from("heap_allocate")).func_body().local_get(class);

            let id = heap_allocate.finish(vec![class], &mut emitter.module.funcs);

            emitter.method_function_map.insert(String::from("allocate_class"), MethodFunctionType::Normal(id));
        }

        emitter
    }

    pub fn build(mut self) -> Vec<u8> {
        self.module.emit_wasm()
    }

    pub fn process_classes(&mut self) {
        for (classpath, class) in self.class_loader.class_map.iter() {
            let offset = self.memory.append_resource(ResourceType::String(classpath.clone()));

            let clazz = class.unwrap();

            self.memory.append_resource(ResourceType::Class {
                classpath_offset: 0,
                fields: Vec::new(),
                fields_length: clazz.full_heap_size as i32
            });

            self.process_class(clazz.as_ref());
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

        if let AttributeItem::Code(code) = &method.attribute_map.get("Code")?.info {
            let params_vec: Vec<ValType> = method_descriptor.parameters.iter().map(|p| field_descriptor_as_value_type(p)).collect();
            let return_vec = match &method_descriptor.return_type {
                MethodReturnType::Void => vec![],
                MethodReturnType::FieldDescriptor(f) => vec![field_descriptor_as_value_type(f)]
            };

            let mut builder = FunctionBuilder::new(
                &mut self.module.types,
                &params_vec[..],
                &return_vec[..]
            );

            let mut locals = Self::analyze_locals(code, class);

            let mut java_locals = HashMap::new();

            let mut min_local_index = 0;
            let mut max_local_index = 0;

            for local in locals.iter() {
                if *local.0 < min_local_index { min_local_index = *local.0; }
                if *local.0 > max_local_index { max_local_index = *local.0; }
            }

            let mut i: usize = 0;

            for param in method_descriptor.parameters.iter() {
                java_locals.insert(i, self.module.locals.add(field_descriptor_as_value_type(param)));

                i += 1;
            }

            let local_variables_not_params = max(max_local_index, method_descriptor.parameters.len()) - method_descriptor.parameters.len();

            println!("Max index: {}, local vars not params: {}", max_local_index, local_variables_not_params);

            for index in max_local_index-local_variables_not_params..max_local_index {
                java_locals.insert(index, self.module.locals.add(locals.get(&index).unwrap().clone()));
            }

            self.bytecode_to_body(&mut builder, code, method, class, java_locals);

            builder.name(
                formatted
            );

            let id = builder.finish(
                method_descriptor.parameters.iter().map(|x| self.module.locals.add(field_descriptor_as_value_type(x))).collect(),
                &mut self.module.funcs
            );

            self.method_function_map.insert(format_method_name(class, method), MethodFunctionType::Normal(id));


        } else {
            //Native method (but we should probably check the flags as well but eh)

            //Native methods are treated as an imported external function

            let params_vec: Vec<ValType> = method_descriptor.parameters.iter().map(|x| field_descriptor_as_value_type(x)).collect();
            let return_vec = match &method_descriptor.return_type {
                MethodReturnType::Void => vec![],
                MethodReturnType::FieldDescriptor(fd) => vec![field_descriptor_as_value_type(fd)]
            };

            let native_type = self.module.types.add(
                &params_vec[..],
                &return_vec[..]
            );

            let import = self.module.add_import_func(
                "jvm_native",
                &formatted,
                native_type
            );

            self.method_function_map.insert(String::from(&formatted), MethodFunctionType::NativeImport(import.0, import.1));
        }

        Option::Some(())
    }

    ///Returns every single local variable that is used and it's type
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
                0x1e..=0x21 => { //iload_<n>
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
                _ => {}
            }
        }

        jvm_locals
    }

    fn bytecode_to_body(&mut self, builder: &mut FunctionBuilder, code: &Code, method: &Method, class: &Class, java_locals: HashMap<usize, LocalId>) -> Option<()> {
        let mut bytes = Cursor::new(code.code.clone());
        let len = code.code.len();

        let mut locals_max = 0; //The max index used to the local map in the bytecode

        let mut helper = StackHelper::new(&mut self.module.locals);

        java_locals.iter().for_each(|i| {
            if *i.0 > locals_max { locals_max = *i.0; }
        });
        
        let mut body = builder.func_body();

        while bytes.position() < len as u64 {
            let opcode = bytes.read_u8().unwrap();

            match opcode {
                0x1 => { //aconst_null
                    body.i32_const(self.memory.null as i32);
                },
                0x2..=0x5 => { //iconst_<n-1>
                    body.i32_const(opcode as i32 - 0x3);
                },
                0x10 => {
                    body.i32_const(bytes.read_u8().unwrap() as i32);
                },
                0x12 => { //ldc
                    let index = bytes.read_u8().unwrap();
                    
                    match class.constant_pool.get(index as usize).unwrap() {
                        Constant::Integer(int) => { body.i32_const(*int); },
                        Constant::Float(float) => { body.f32_const(*float); },
                        Constant::Long(long) => { body.i64_const(*long); },
                        Constant::Double(double) => { body.f64_const(*double as f64); },
                        Constant::String(string) => {
                            let utf8 = class.constant_pool.resolve_utf8(*string).unwrap();

                            let offset = if !self.memory.strings.contains_key(utf8) {
                                self.memory.allocate_string(String::from(utf8), self.class_loader.get_class("java/lang/String").unwrap().as_ref())
                            } else {
                                *self.memory.strings.get(utf8).unwrap()
                            };

                            body.i32_const(offset as i32);
                        }
                        _ => {}
                    };
                },
                0x15..=0x19 => { //iload, lload, fload, dload, aload
                    body.local_get(java_locals.get((&(bytes.read_u8().unwrap() as usize)))?.clone());
                },
                0x1a..=0x1d => { //iload_<n>
                    body.local_get(java_locals.get(&(opcode as usize - 0x1))?.clone());
                },
                0x1e..=0x21 => { //lload_<n>
                    body.local_get(java_locals.get(&(opcode as usize - 0x1e))?.clone());
                },
                0x22..=0x25 => { //fload_<n>
                    body.local_get(java_locals.get(&(opcode as usize - 0x22))?.clone());
                },
                0x26..=0x29 => { //dload_<n>
                    body.local_get(java_locals.get(&(opcode as usize - 0x26))?.clone());
                },
                0x2a..=0x2d => { //aload_<n>
                    body.local_get(java_locals.get(&(opcode as usize - 0x2a))?.clone());
                },
                0x32 => { //aaload
                    body.binop(BinaryOp::I32Add);
                    body.store(
                        self.memory.memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                0x33 => { //baload
                    body.i32_const(1);
                    body.binop(BinaryOp::I32Add);

                    body.binop(BinaryOp::I32Add); //Increment by one

                    //Arrayref is now at the top of the stack
                    body.binop(BinaryOp::I32Add);
                    //The proper offset to the element is now at the top
                    body.store(
                        self.memory.memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    ); //Load the byte/bool
                },
                0x3a => { //astore
                    body.local_set(java_locals.get(&(bytes.read_u8().unwrap() as usize))?.clone());
                },
                0x3b..=0x3e => { //istore_<n>
                    body.local_set(java_locals.get(&(opcode as usize - 0x3b))?.clone());
                },
                0x4b..=0x4e => { //astore_<n>
                    body.local_set(java_locals.get(&(opcode as usize - 0x4b))?.clone());
                },
                0x53 => { //aastore
                    body.local_set(helper.get(ValType::I32, 0));
                    body.binop(BinaryOp::I32Add);
                    body.local_get(helper.get(ValType::I32, 0));
                    body.store(
                        self.memory.memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                0x54 => { //bastore
                    body.local_set(helper.get(ValType::I32, 0));

                    //arrayref is on top

                    //TODO: null check

                    body.i32_const(1); //Increment by one because the length is first
                    body.binop(BinaryOp::I32Add); //index + 1
                    body.binop(BinaryOp::I32Add); //(index + 1) + arrayref

                    body.local_get(helper.get(ValType::I32, 0)); //Un-stash the value
                    body.store(
                        self.memory.memory_id,
                        StoreKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    );
                },
                0xac => {
                    body.return_();
                }
                0xb0 => {
                    body.return_();
                },
                0xb1 => { //return (void)
                    body.return_();
                },
                0xb5 => { //putfield
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    if let Constant::FieldRef(class_index, nat_index) = class.constant_pool.get(index as usize).unwrap() {
                        let clazz = class.constant_pool.resolve_class_info(*class_index).unwrap();
                        let (name, descriptor) = class.constant_pool.resolve_name_and_type(*nat_index).unwrap();

                        let field_offset = class.field_map.get(name).unwrap().offset;
                        let fd = FieldDescriptor::parse(descriptor);

                        body.local_set(
                            helper.get(ValType::I32, 0)
                        ); //stash

                        body.i32_const(field_offset as i32);
                        body.i32_const(1);

                        body.binop(BinaryOp::I32Add);
                        body.binop(BinaryOp::I32Add);

                        body.local_get(
                            helper.get(ValType::I32, 0)
                        );
                    } else { panic!("Constant did not resolve to FieldRef!"); }
                },
                0xb7 => { //TODO: invokespecial
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    //nop
                },
                0xb8 => { //TODO: invokestatic
                    let index = bytes.read_u16::<BigEndian>().unwrap();

                    //nop
                },
                0xbd => { //anewarray
                    body.i32_const(1); //Size of the array takes up an I32
                    body.binop(BinaryOp::I32Add);
                    body.call(
                        match self.method_function_map.get("allocate_to_heap").unwrap() {
                            MethodFunctionType::Normal(funcid) => funcid.clone(),
                            MethodFunctionType::NativeImport(_, _) => unreachable!()
                        }
                    );
                },
                0xbe => { //arraylength
                    body.load(
                        self.memory.memory_id,
                        LoadKind::I32 {
                            atomic: true
                        },
                        MemArg {
                            align: 1,
                            offset: 0
                        }
                    ); //the pointer to the array will be the length itself
                },
                _ => unimplemented!("Unimplemented opcode {}", opcode)
            }
        }

        Option::Some(())
    }
}
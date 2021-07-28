use crate::vm::class::attribute::Attribute;
use crate::vm::class::constant::Constant;
use crate::vm::class::FieldDescriptor;
use crate::vm::class::{
    BaseType, Class, ConstantPool, FieldInfo, Method, MethodDescriptor, MethodReturnType,
    ObjectField,
};
use byteorder::{BigEndian, ReadBytesExt};
use std::collections::HashMap;
use std::io::Cursor;

use std::iter::FromIterator;
use std::ops::Deref;
use std::path::PathBuf;
use std::{fs, io};

use crate::vm::vm::JvmError;
use std::mem::size_of;
use std::sync::Arc;

#[derive(Debug)]
pub enum ClassLoadState {
    NotLoaded,
    Loading,
    Loaded(Arc<Class>),
    DeserializationError(DeserializationError),
}

impl ClassLoadState {
    pub fn unwrap(&self) -> Arc<Class> {
        match self {
            ClassLoadState::NotLoaded => panic!("Cannot unwrap an unloaded class."),
            ClassLoadState::Loading => panic!("Cannot unwrap a class that is being loaded."),
            ClassLoadState::Loaded(class) => class.clone(),
            ClassLoadState::DeserializationError(_) => {
                panic!("Cannot unwrap a class that failed to load.")
            }
        }
    }
}

impl From<DeserializationError> for ClassLoadState {
    fn from(e: DeserializationError) -> Self {
        Self::DeserializationError(e)
    }
}

pub struct ClassLoader {
    pub class_map: HashMap<String, ClassLoadState>,

    classpath_root: PathBuf,
}

impl ClassLoader {
    pub fn new(classpath_root: PathBuf) -> Self {
        Self {
            class_map: HashMap::new(),
            classpath_root,
        }
    }

    pub fn get_class(&self, classpath: &str) -> Option<Arc<Class>> {
        Option::Some(self.class_map.get(classpath)?.unwrap())
    }

    pub fn is_class_linked(&self, classpath: &str) -> bool {
        let class = self.class_map.get(classpath);
        if class.is_none() {
            false
        } else {
            if let ClassLoadState::Loaded(_) = class.unwrap() {
                true
            } else {
                false
            }
        }
    }

    fn load_link_field_descriptor(&mut self, fd: &FieldDescriptor) {
        match fd {
            FieldDescriptor::BaseType(_) => {}
            FieldDescriptor::ObjectType(obj_classpath) => {
                self.load_and_link_class(&obj_classpath);
            }
            FieldDescriptor::ArrayType(at) => match &*at.field_descriptor {
                FieldDescriptor::ObjectType(obj_classpath) => {
                    self.load_and_link_class(&obj_classpath);
                }
                _ => {}
            },
        }
    }

    pub fn load_and_link_class(&mut self, classpath: &str) -> Result<(bool, Arc<Class>), JvmError> {
        let maybe = self.class_map.get(classpath);

        match maybe {
            Some(_) => {
                match maybe.unwrap() {
                    ClassLoadState::NotLoaded => {}
                    ClassLoadState::Loading => {
                        return Result::Err(JvmError::ClassLoadError(ClassLoadState::Loading))
                    }
                    ClassLoadState::Loaded(_) => {
                        return Result::Ok((false, maybe.unwrap().unwrap()))
                    }
                    ClassLoadState::DeserializationError(e) => {
                        return Result::Err(JvmError::ClassLoadError(
                            ClassLoadState::DeserializationError(e.clone()),
                        ))
                    }
                };
            }
            None => {}
        };

        let split_classpath = classpath.split("/");
        let mut physical_classpath = PathBuf::new();

        for x in split_classpath {
            physical_classpath = physical_classpath.join(x);
        }
        physical_classpath.set_extension("class");

        let real_path = self.classpath_root.join(physical_classpath);

        let bytes = fs::read(real_path).unwrap();

        // self.loaded_classes.insert(String::from(classpath), true);
        self.class_map
            .insert(String::from(classpath), ClassLoadState::Loading);

        let mut class = load_class(bytes)?;

        if !self.is_class_linked(&class.super_class) {
            if class.super_class != "" {
                self.load_and_link_class(&*String::from(&class.super_class));
            }
        }

        if class.super_class != "" {
            // println!("Getting superclass {} of {}", &class.super_class, classpath);
            let superclass = self.get_class(&class.super_class).unwrap();
            class.full_heap_size = class.heap_size + superclass.full_heap_size;
        }

        for constant in class.constant_pool.get_vec().iter() {
            match constant {
                Constant::Class(utf8) => {
                    let name = class.constant_pool.resolve_utf8(*utf8).unwrap();
                    self.load_and_link_class(name);
                }
                Constant::FieldRef(class_name, name_and_type) => {
                    self.load_and_link_class(
                        class.constant_pool.resolve_class_info(*class_name).unwrap(),
                    );

                    let (_, descriptor) = class
                        .constant_pool
                        .resolve_name_and_type(*name_and_type)
                        .unwrap();
                    let fd = FieldDescriptor::parse(descriptor)?;

                    self.load_link_field_descriptor(&fd);
                }
                Constant::MethodRef(class_name, name_and_type) => {
                    self.load_and_link_class(
                        class.constant_pool.resolve_class_info(*class_name).unwrap(),
                    );

                    let (_, descriptor) = class
                        .constant_pool
                        .resolve_name_and_type(*name_and_type)
                        .unwrap();
                    let md = MethodDescriptor::parse(descriptor)?;

                    match md.return_type {
                        MethodReturnType::Void => {}
                        MethodReturnType::FieldDescriptor(fd) => {
                            self.load_link_field_descriptor(&fd)
                        }
                    }

                    for param in md.parameters.iter() {
                        self.load_link_field_descriptor(param);
                    }
                }
                Constant::InterfaceMethodRef(_, _) => {}
                Constant::NameAndType(_, _) => {}
                Constant::MethodHandle(_, _) => {}
                Constant::MethodType(_) => {}
                Constant::InvokeDynamic(_, _) => {}
                _ => {}
            }
        }

        self.class_map.insert(
            String::from(classpath),
            ClassLoadState::Loaded(Arc::new(class)),
        );

        let rc = self.class_map.get(classpath).unwrap().unwrap();

        for (_, info) in rc.field_map.iter() {
            match &info.info.field_descriptor {
                FieldDescriptor::ObjectType(fd_classpath) => {
                    self.load_and_link_class(fd_classpath);
                }
                FieldDescriptor::ArrayType(array_type) => {
                    if let FieldDescriptor::ObjectType(fd_classpath) =
                        array_type.field_descriptor.deref()
                    {
                        self.load_and_link_class(&fd_classpath);
                    }
                }
                FieldDescriptor::BaseType(_) => {}
            }
        }

        Result::Ok((true, rc))
    }

    pub fn recurse_is_superclass(&self, subclass: &Class, superclass_cpath: &str) -> bool {
        if subclass.this_class == superclass_cpath {
            false
        } else if superclass_cpath == "java/lang/Object"
            && subclass.this_class != "java/lang/Object"
        {
            true
        } else if subclass.super_class == superclass_cpath {
            true
        } else if subclass.super_class == "java/lang/Object"
            && superclass_cpath != "java/lang/Object"
        {
            false
        } else {
            let superclass = self.get_class(&subclass.super_class).unwrap();
            self.recurse_is_superclass(superclass.as_ref(), superclass_cpath)
        }
    }

    pub fn recurse_resolve_overridding_method(
        &self,
        subclass: Arc<Class>,
        name: &str,
        descriptor: &str,
    ) -> Option<(Arc<Class>, Arc<Method>)> {
        let superclass = self.get_class(&subclass.super_class).unwrap();

        if subclass.has_method(name, descriptor) {
            let m1 = subclass.get_method(name, descriptor).ok()?;
            let m2 = superclass.get_method(name, descriptor).ok()?;

            if {
                (m2.access_flags & 0x1 == 0x1)
                    || (m2.access_flags & 0x4 == 0x4)
                    || (!((m2.access_flags & 0x1 == 0x1)
                        && (m2.access_flags & 0x2 == 0x2)
                        && (m2.access_flags & 0x4 == 0x4))
                        && are_classpaths_siblings(&subclass.this_class, &superclass.this_class))
            } {
                return Option::Some((subclass, m1));
            } else {
                if superclass.super_class != "" {
                    return self.recurse_resolve_overridding_method(superclass, name, descriptor);
                } else {
                    //No superclass
                    return Option::None;
                }
            }
        }

        return Option::None;
    }

    pub fn recurse_resolve_supermethod_special(
        &self,
        subclass: Arc<Class>,
        name: &str,
        descriptor: &str,
    ) -> Option<(Arc<Class>, Arc<Method>)> {
        if subclass.super_class == "" {
            return Option::None;
        }

        let superclass = self.get_class(&subclass.super_class)?;

        if superclass.has_method(name, descriptor) {
            Option::Some((
                superclass.clone(),
                superclass.get_method(name, descriptor).ok()?,
            ))
        } else {
            self.recurse_resolve_supermethod_special(superclass, name, descriptor)
        }
    }
}

pub fn are_classpaths_siblings(a: &str, b: &str) -> bool {
    let split_a = Vec::from_iter(a.split("/").map(String::from));
    let split_b = Vec::from_iter(b.split("/").map(String::from));

    if split_a.len() != split_b.len() {
        false
    } else {
        for elem in 0..split_a.len() - 1 {
            if split_a.get(elem).unwrap() != split_b.get(elem).unwrap() {
                return false;
            }
        }

        true
    }
}

#[derive(Debug)]
pub enum DeserializationError {
    InvalidConstant(String),
    ClassFormatError,
    MajorTooHigh(u16),
    EOF,
    InvalidPayload,
}

impl Clone for DeserializationError {
    fn clone(&self) -> Self {
        match self {
            DeserializationError::InvalidConstant(s) => Self::InvalidConstant(s.clone()),
            DeserializationError::ClassFormatError => Self::ClassFormatError,
            DeserializationError::MajorTooHigh(m) => Self::MajorTooHigh(*m),
            DeserializationError::EOF => Self::EOF,
            DeserializationError::InvalidPayload => Self::InvalidPayload,
        }
    }
}

impl From<io::Error> for DeserializationError {
    fn from(_: io::Error) -> Self {
        Self::EOF
    }
}

pub fn load_class(bytes: Vec<u8>) -> Result<Class, DeserializationError> {
    let mut rdr = Cursor::new(bytes);

    let magic = rdr.read_u32::<BigEndian>()?;

    if magic != 0xCAFEBABE {
        return Result::Err(DeserializationError::ClassFormatError);
    }

    let major = rdr.read_u16::<BigEndian>()?;
    let _minor = rdr.read_u16::<BigEndian>()?;

    if major > 51 {
        // Major version too high for this JVM implementation
        return Result::Err(DeserializationError::MajorTooHigh(51));
    }

    let constant_pool_count = rdr.read_u16::<BigEndian>()?; //The constant pool count is equal to the number of constants + 1

    let mut constant_pool: ConstantPool = ConstantPool::new();

    constant_pool.push(Constant::Utf8(String::from(""))); //To get the index to 1

    for _ in 1..constant_pool_count {
        let constant = Constant::from_bytes(&mut rdr, &constant_pool).map_err(|_| {
            DeserializationError::InvalidConstant(String::from(
                "Couldn't deserialize constant from constant pool",
            ))
        })?;

        constant_pool.push(constant);
    }

    let access_flags = rdr.read_u16::<BigEndian>()?;

    let this_class_index = rdr.read_u16::<BigEndian>()?;
    let this_class: String;

    let super_class_index = rdr.read_u16::<BigEndian>()?;

    let super_class: String;

    this_class = String::from(constant_pool.resolve_class_info(this_class_index).ok_or(
        DeserializationError::InvalidConstant(String::from("this_class constant was not a UTF8!")),
    )?);

    match constant_pool.resolve_class_info(super_class_index) {
        Some(name) => {
            super_class = String::from(name);
        }
        None => {
            if this_class != "java/lang/Object" {
                return Result::Err(DeserializationError::InvalidConstant(format!(
                    "Superclass must be a ClassInfo constant! {:?}",
                    constant_pool.get(super_class_index as usize).unwrap()
                )));
            } else {
                super_class = String::from(""); // java/lang/Object does not have a superclass.
            }
        }
    }

    let interfaces_count = rdr.read_u16::<BigEndian>()?;

    let interfaces: Vec<u16> = (0..interfaces_count)
        .map(|_| rdr.read_u16::<BigEndian>().unwrap())
        .collect();

    let field_count = rdr.read_u16::<BigEndian>()?;

    let mut field_map: HashMap<String, ObjectField> = HashMap::new();

    let mut old_offset = 0;
    let mut new_offset = 0;

    let mut heap_size: usize = 0;

    for _ in 0..field_count {
        let field = FieldInfo::from_bytes(&mut rdr, &constant_pool)
            .map_err(|_| DeserializationError::InvalidPayload)?;

        let size = (match &field.field_descriptor {
            FieldDescriptor::BaseType(b_type) => BaseType::size_of(b_type),
            _ => size_of::<usize>(),
        }) as isize;

        heap_size += size as usize;
        new_offset += size;

        field_map.insert(
            field.name.clone(),
            ObjectField {
                offset: old_offset,
                info: field,
            },
        );

        old_offset = new_offset;
    }

    let method_count = rdr.read_u16::<BigEndian>()?;

    let mut method_map: HashMap<(String, String), Arc<Method>> = HashMap::new();

    for _ in 0..method_count {
        let m = Method::from_bytes(&mut rdr, &constant_pool)
            .map_err(|_| DeserializationError::InvalidPayload)?;

        method_map.insert((m.name.clone(), m.descriptor.clone()), Arc::new(m));
    }

    let attribute_count = rdr.read_u16::<BigEndian>()?;
    // let mut attribute_map: HashMap<&str, Attribute> = HashMap::new();

    let attribute_map: HashMap<String, Attribute> = (0..attribute_count)
        .map(|_| {
            let a = Attribute::from_bytes(&mut rdr, &constant_pool).unwrap();
            (String::from(&a.attribute_name), a)
        })
        .collect();

    Result::Ok(Class {
        constant_pool,
        access_flags,
        this_class,
        super_class,
        interfaces,
        field_map,
        method_map,
        attribute_map,
        heap_size,
        full_heap_size: heap_size,
    })
}

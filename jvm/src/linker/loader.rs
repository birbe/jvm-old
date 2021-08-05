use byteorder::{BigEndian, ReadBytesExt};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::iter::FromIterator;
use std::io;
use std::mem::size_of;
use std::sync::Arc;
use crate::class::{Class, FieldDescriptor, Method, ConstantPool, ObjectField, FieldInfo, JavaType, AccessFlags};
use crate::JvmError;
use std::ops::Deref;
use crate::class::constant::Constant;
use crate::class::attribute::Attribute;

///A trait that provides classes to a VirtualMachine. It is up to the user to implement this.
///An example of this would be a file-system backed implementation.
pub trait ClassProvider: Send + Sync {
    fn get_classfile(&self, classpath: &str) -> Option<&[u8]>;
}

#[derive(Debug)]
pub enum ClassLoadState {
    NotLoaded,
    Loading,
    Loaded(Arc<Class>),
    DeserializationError(DeserializationError),
    NotFound,
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
            ClassLoadState::NotFound => panic!("Could not find the requested classfile."),
        }
    }
}

impl From<DeserializationError> for ClassLoadState {
    fn from(e: DeserializationError) -> Self {
        Self::DeserializationError(e)
    }
}

pub struct ClassLoadReport {
    pub just_loaded: bool,
    pub class: Arc<Class>,
    pub all_loaded_classes: HashSet<Arc<Class>>
}

pub struct ClassLoader {
    pub class_map: HashMap<String, ClassLoadState>,
    pub class_provider: Box<dyn ClassProvider>,
}

impl ClassLoader {
    pub fn new(class_provider: Box<dyn ClassProvider>) -> Self {
        Self {
            class_map: HashMap::new(),
            class_provider,
        }
    }

    pub fn get_class(&self, classpath: &str) -> Option<Arc<Class>> {
        Option::Some(self.class_map.get(classpath)?.unwrap())
    }

    pub fn is_class_linked(&self, classpath: &str) -> bool {
        let class = self.class_map.get(classpath);

        matches!(class, Some(ClassLoadState::Loaded(_)))
    }

    fn load_link_field_descriptor(&mut self, fd: &FieldDescriptor) -> Result<Option<ClassLoadReport>, JvmError> {
        Result::Ok(match fd {
            FieldDescriptor::JavaType(_) => Option::None,
            FieldDescriptor::ObjectType(obj_classpath) => {
                Option::Some(self.load_and_link_class(obj_classpath)?)
            }
            FieldDescriptor::ArrayType(at) => {
                if let FieldDescriptor::ObjectType(obj_classpath) = &*at.field_descriptor {
                    Option::Some(self.load_and_link_class(obj_classpath)?)
                } else {
                    Option::None
                }
            }
        })
    }

    // fn handle_load_link_fd(&mut self, fd: &FieldDescriptor, classes: &mut HashSet<Arc<Class>>) -> Result<(), JvmError> {
    //     match self.load_link_field_descriptor(fd) {
    //         Ok(opt) =>
    //             match opt {
    //                 None => {}
    //                 Some(report) => classes.extend(report.all_loaded_classes.into_iter()),
    //             },
    //         Err(err) => match err {
    //             JvmError::ClassLoadError(class_load_err) => match class_load_err {
    //                 ClassLoadState::Loading | ClassLoadState::Loaded(_) => {},
    //                 _ => return Result::Err(JvmError::ClassLoadError(class_load_err))
    //             },
    //             _ => return Result::Err(err)
    //         }
    //     }
    //
    //     Result::Ok(())
    // }

    fn handle_class_report(&mut self, classpath: &str, classes: &mut HashSet<Arc<Class>>) -> Result<(), JvmError> {
        match self.load_and_link_class(classpath) {
            Ok(report) => {
                classes.extend(report.all_loaded_classes.into_iter());
            }
            Err(err) => match err {
                JvmError::ClassLoadError(class_load_err) => match class_load_err {
                    ClassLoadState::Loading | ClassLoadState::Loaded(_) => {},
                    _ => return Result::Err(JvmError::ClassLoadError(class_load_err))
                },
                _ => return Result::Err(err)
            }
        }

        Result::Ok(())
    }

    ///Load a class and it's superclasses
    pub fn load_and_link_class(&mut self, classpath: &str) -> Result<ClassLoadReport, JvmError> {
        let class_load_state = self.class_map.get(classpath);
        let mut all_loaded_classes = HashSet::new();

        if let Some(class_load_state) = class_load_state {
            match class_load_state {
                ClassLoadState::NotLoaded => {}
                ClassLoadState::Loading => {
                    return Result::Err(JvmError::ClassLoadError(ClassLoadState::Loading))
                }
                ClassLoadState::Loaded(class) => return Result::Ok(ClassLoadReport {
                    just_loaded: false,
                    class: class.clone(),
                    all_loaded_classes: HashSet::new()
                }),
                ClassLoadState::DeserializationError(e) => {
                    return Result::Err(JvmError::ClassLoadError(
                        ClassLoadState::DeserializationError(e.clone()),
                    ))
                }
                ClassLoadState::NotFound => {
                    return Result::Err(JvmError::ClassLoadError(ClassLoadState::NotFound))
                }
            };
        };

        let bytes = self
            .class_provider
            .get_classfile(classpath)
            .ok_or(JvmError::ClassLoadError(ClassLoadState::NotFound))?;

        // self.loaded_classes.insert(String::from(classpath), true);
        self.class_map
            .insert(String::from(classpath), ClassLoadState::Loading);

        let mut class = deserialize_class(bytes)?;

        match &class.super_class {
            None => {}
            Some(superclass) => {
                if !self.is_class_linked(superclass) {
                    self.handle_class_report(superclass, &mut all_loaded_classes)?;
                }

                let superclass = self.get_class(superclass).unwrap();
                class.full_heap_size = class.heap_size + superclass.full_heap_size;
            }
        }

        self.class_map.insert(
            String::from(classpath),
            ClassLoadState::Loaded(Arc::new(class)),
        );

        let rc = self.class_map.get(classpath).unwrap().unwrap();

        for (_, info) in rc.field_map.iter() {
            match &info.info.field_descriptor {
                FieldDescriptor::ObjectType(fd_classpath) =>
                    self.handle_class_report(fd_classpath, &mut all_loaded_classes)?,
                FieldDescriptor::ArrayType(array_type) => {
                    if let FieldDescriptor::ObjectType(fd_classpath) =
                        array_type.field_descriptor.deref()
                    {
                        self.handle_class_report(fd_classpath, &mut all_loaded_classes)?
                    }
                }
                FieldDescriptor::JavaType(_) => {}
            }
        }

        all_loaded_classes.insert(rc.clone());

        Result::Ok(ClassLoadReport {
            just_loaded: true,
            class: rc,
            all_loaded_classes
        })
    }

    pub fn recurse_is_superclass(&self, subclass: Arc<Class>, superclass: Arc<Class>) -> bool {
        if subclass == superclass {
            true
        } else if superclass.this_class == "java/lang/Object" {
            true
        } else if subclass.this_class == "java/lang/Object" {
            false
        } else {
            let superclass_of_subclass = self.get_class(subclass.super_class.as_deref().unwrap()).unwrap();
            self.recurse_is_superclass(superclass_of_subclass, superclass)
        }
    }

    pub fn recurse_resolve_overridding_method(
        &self,
        subclass: Arc<Class>,
        name: &str,
        descriptor: &str,
    ) -> Option<(Arc<Class>, Arc<Method>)> {
        let superclass = self.get_class(&subclass.super_class.as_ref().unwrap()).unwrap();

        if subclass.has_method(name, descriptor) {
            let m1 = subclass.get_method(name, descriptor).ok()?;
            let m2 = superclass.get_method(name, descriptor).ok()?;

            if (m2.access_flags & 0x1 == 0x1)
                || (m2.access_flags & 0x4 == 0x4)
                || are_classpaths_siblings(&subclass.this_class, &superclass.this_class)
            {
                return Option::Some((subclass, m1));
            } else if !superclass.super_class.as_deref().unwrap_or("").is_empty() {
                return self.recurse_resolve_overridding_method(superclass, name, descriptor);
            } else {
                //No superclass
                return Option::None;
            }
        }

        Option::None
    }

    pub fn recurse_resolve_supermethod_special(
        &self,
        subclass: Arc<Class>,
        name: &str,
        descriptor: &str,
    ) -> Option<(Arc<Class>, Arc<Method>)> {
        if subclass.super_class.as_deref().unwrap().is_empty() {
            return Option::None;
        }

        let superclass = self.get_class(subclass.super_class.as_deref().unwrap())?;

        if superclass.has_method(name, descriptor) {
            Option::Some((
                superclass.clone(),
                superclass.get_method(name, descriptor).ok()?,
            ))
        } else {
            self.recurse_resolve_supermethod_special(superclass, name, descriptor)
        }
    }

    fn does_interface_implement_interface(&self, sub_interface: Arc<Class>, super_interface: Arc<Class>) -> bool {
        //Speaks for itself
        if sub_interface == super_interface {
            true
        } else if sub_interface.super_class.as_deref().unwrap() == super_interface.this_class {
            //If the subclass extends the superclass
            true
        } else {
            //Get the implemented interfaces of the subclass
            let interfaces = &sub_interface.interfaces;

            for class_index in interfaces {
                let implementing_name = sub_interface.constant_pool
                    .resolve_class_info(*class_index)
                    .unwrap();

                let implementing = self.get_class(implementing_name)
                    .unwrap();

                //Resolve the classes

                if implementing == super_interface {
                    return true;
                } else {
                    //Recursively check if it's implemented
                    if self.does_interface_implement_interface(
                        implementing.clone(),
                        super_interface.clone())
                        | self.recurse_is_superclass(
                        implementing.clone(),
                        super_interface.clone()) {

                        return true;
                    }
                }
            }

            false
        }
    }

    pub fn check_cast(
        &self,
        s_class: Arc<Class>,
        t_class: Arc<Class>
    ) -> bool {
        //If S is an interface class
        if s_class.access_flags & AccessFlags::INTERFACE.bits() == AccessFlags::INTERFACE.bits() {
            return self.does_interface_implement_interface(t_class, s_class)
        } else {
            //S is a normal class
            return self.recurse_is_superclass(t_class, s_class);
        }

        false
    }
}

pub fn are_classpaths_siblings(a: &str, b: &str) -> bool {
    let split_a = Vec::from_iter(a.split('/').map(String::from));
    let split_b = Vec::from_iter(b.split('/').map(String::from));

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

pub fn deserialize_class(bytes: &[u8]) -> Result<Class, DeserializationError> {
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

    let super_class: Option<String>;

    this_class = String::from(
        constant_pool
            .resolve_class_info(this_class_index)
            .ok_or_else(|| {
                DeserializationError::InvalidConstant(String::from(
                    "this_class constant was not a UTF8!",
                ))
            })?,
    );

    match constant_pool.resolve_class_info(super_class_index) {
        Some(name) => {
            super_class = Option::Some(String::from(name));
        }
        None => {
            if this_class != "java/lang/Object" {
                return Result::Err(DeserializationError::InvalidConstant(format!(
                    "Superclass must be a ClassInfo constant! {:?}",
                    constant_pool.get(super_class_index as usize).unwrap()
                )));
            } else {
                super_class = Option::None; // java/lang/Object does not have a superclass.
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
            FieldDescriptor::JavaType(b_type) => JavaType::size_of(b_type),
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

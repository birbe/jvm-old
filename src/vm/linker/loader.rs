use std::io::Cursor;
use byteorder::{ReadBytesExt, BigEndian};
use crate::vm::class::constant::{Constant};
use crate::vm::class::{Class, FieldInfo, Method, ObjectField, BaseType, ConstantPool};
use crate::vm::class::attribute::{Attribute};
use std::collections::HashMap;
use crate::vm::class::FieldDescriptor;
use std::mem::size_of;

pub fn load_class(bytes: Vec<u8>) -> Class {
    let mut rdr = Cursor::new(bytes);

    let magic = rdr.read_u32::<BigEndian>().unwrap();

    if magic != 0xCAFEBABE {
        panic!("ClassFormatError");
    }

    let major = rdr.read_u16::<BigEndian>().unwrap();
    let minor = rdr.read_u16::<BigEndian>().unwrap();

    if major > 51 {
        panic!("Major version too high for this JVM implementation. Aborting");
    }

    let constant_pool_count = rdr.read_u16::<BigEndian>().unwrap(); //The constant pool count is equal to the number of constants + 1

    let mut constant_pool: ConstantPool = ConstantPool::new();

    constant_pool.push(Constant::Utf8(String::from(""))); //To get the index to +1

    for _ in 1..constant_pool_count {
        let constant = Constant::from_bytes(&mut rdr);

        constant_pool.push(constant);
    }

    let access_flags = rdr.read_u16::<BigEndian>().unwrap();

    let this_class_index = rdr.read_u16::<BigEndian>().unwrap();
    let this_class: String;

    let super_class_index = rdr.read_u16::<BigEndian>().unwrap();

    let super_class: String;

    if let Constant::Class(const_index) = constant_pool.get(this_class_index as usize).unwrap() {
        if let Constant::Utf8(string) = constant_pool.get(*const_index as usize).unwrap() {
            this_class = String::from(string);
        } else {
            unreachable!("?? what??? the fuck");
        }
    } else {
        panic!("This-class must be a ClassInfo constant!");
    }

    if let Constant::Class(const_index) = constant_pool.get(super_class_index as usize).unwrap() {
        if let Constant::Utf8(string) = constant_pool.get(*const_index as usize).unwrap() {
            super_class = String::from(string);
        } else {
            unreachable!("?? what??? the fuck");
        }
    } else {
        if this_class != "java/lang/Object" {
            panic!(format!("Superclass must be a ClassInfo constant! {:?}", constant_pool.get(super_class_index as usize).unwrap()));
        } else {
            super_class = String::from(""); //Object does not have a superclass.
        }
    }


    let interfaces_count = rdr.read_u16::<BigEndian>().unwrap();

    let interfaces: Vec<u16> = (0..interfaces_count)
        .map(|_| rdr.read_u16::<BigEndian>().unwrap())
        .collect();

    let field_count = rdr.read_u16::<BigEndian>().unwrap();

    let mut field_map: HashMap<String, ObjectField> = HashMap::new();

    let mut old_offset = 0;
    let mut new_offset = 0;

    if this_class == "java/lang/String" && field_count == 0 {
        panic!("java/lang/String had no fields");
    }

    for _ in 0..field_count {
        let field = FieldInfo::from_bytes(&mut rdr, &constant_pool);

        unsafe {
            new_offset += match &field.field_descriptor {
                FieldDescriptor::BaseType(b_type) => BaseType::size_of(b_type),
                FieldDescriptor::ObjectType(_) => size_of::<usize>(),
                FieldDescriptor::ArrayType(_) => size_of::<usize>()
            } as isize;
        }

        field_map.insert(field.name.clone(), ObjectField {
            offset: old_offset,
            info: field
        });

        old_offset = new_offset;
    }

    let method_count = rdr.read_u16::<BigEndian>().unwrap();

    // let method_map: HashMap<String, HashMap<String, Method>> = (0..method_count).map( |_| {
    //     let m = Method::from_bytes(&mut rdr, &constant_pool);
    //     (String::from(&m.name), HashMap::new())
    // }).collect();

    let mut method_map: HashMap<String, HashMap<String, Method>> = HashMap::new();

    for _ in 0..method_count {
        let m = Method::from_bytes(&mut rdr, &constant_pool);

        if !method_map.contains_key(m.name.as_str()) {
            method_map.insert(m.name.clone(), HashMap::new());
        }

        method_map.get_mut(m.name.clone().as_str())
            .unwrap()
            .insert(String::from(&m.descriptor), m);
    }

    let attribute_count = rdr.read_u16::<BigEndian>().unwrap();
    // let mut attribute_map: HashMap<&str, Attribute> = HashMap::new();

    let attribute_map: HashMap<String, Attribute> = (0..attribute_count).map( |_| {
        let a = Attribute::from_bytes(&mut rdr, &constant_pool);
        (String::from(&a.attribute_name), a)
    }).collect();

    Class {
        constant_pool,
        access_flags,
        this_class,
        super_class,
        interfaces,
        field_map,
        method_map,
        attribute_map
    }
}
use std::io::Cursor;
use byteorder::{ReadBytesExt, BigEndian};
use crate::vm::class::constant::{Constant};
use crate::vm::class::{ClassInfo, FieldInfo, Method};
use crate::vm::class::attribute::{Attribute};
use std::collections::HashMap;

pub fn load_class(bytes: Vec<u8>) -> ClassInfo {
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

    let mut constant_pool: Vec<Constant> = Vec::new();

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

    if let Constant::Class(const_index) = constant_pool.get(super_class_index as usize).unwrap() {
        if let Constant::Utf8(string) = constant_pool.get(*const_index as usize).unwrap() {
            super_class = String::from(string);
        } else {
            unreachable!("?? what??? the fuck");
        }
    } else {
        panic!("Superclass must be a ClassInfo constant!");
    }

    if let Constant::Class(const_index) = constant_pool.get(this_class_index as usize).unwrap() {
        if let Constant::Utf8(string) = constant_pool.get(*const_index as usize).unwrap() {
            this_class = String::from(string);
        } else {
            unreachable!("?? what??? the fuck");
        }
    } else {
        panic!("This-class must be a ClassInfo constant!");
    }

    let interfaces_count = rdr.read_u16::<BigEndian>().unwrap();

    let interfaces: Vec<u16> = (0..interfaces_count)
        .map(|_| rdr.read_u16::<BigEndian>().unwrap())
        .collect();

    let field_count = rdr.read_u16::<BigEndian>().unwrap();
    let fields: Vec<FieldInfo> = (0..field_count)
        .map(|_| FieldInfo::from_bytes(&mut rdr, &constant_pool))
        .collect();

    let method_count = rdr.read_u16::<BigEndian>().unwrap();

    let method_map: HashMap<String, Method> = (0..method_count).map( |_| {
        let m = Method::from_bytes(&mut rdr, &constant_pool);
        (String::from(&m.name), m)
    }).collect();

    let attribute_count = rdr.read_u16::<BigEndian>().unwrap();
    // let mut attribute_map: HashMap<&str, Attribute> = HashMap::new();

    let attribute_map: HashMap<String, Attribute> = (0..attribute_count).map( |_| {
        let a = Attribute::from_bytes(&mut rdr, &constant_pool);
        (String::from(&a.attribute_name), a)
    }).collect();

    ClassInfo {
        magic,
        minor_version: minor,
        major_version: major,
        constant_pool_count,
        constant_pool,
        access_flags,
        this_class,
        super_class,
        interfaces_count,
        interfaces,
        field_count,
        fields,
        method_count,
        method_map,
        attribute_count,
        attribute_map
    }
}
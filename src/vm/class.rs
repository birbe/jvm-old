use crate::vm::class::attribute::Attribute;
use std::any::Any;
use crate::vm::class::constant::Constant;
use std::io::Cursor;
use byteorder::{ReadBytesExt, BigEndian};

pub struct ClassFile {
    pub(crate) magic: u32,
    pub(crate) minor_version: u16,
    pub(crate) major_version: u16,
    pub(crate) constant_pool_count: u16,
    pub(crate) constant_pool: Vec<Constant>,
    pub(crate) access_flags: u16,
    pub(crate) this_class: u16,
    pub(crate) super_class: u16,
    pub(crate) interfaces_count: u16,
    pub(crate) interfaces: Vec<u16>, //Index into the constant pool
    pub(crate) field_count: u16,
    pub(crate) fields: Vec<FieldInfo>,
    pub(crate) method_count: u16,
    pub(crate) methods: Vec<MethodInfo>,
    pub(crate) attribute_count: u16,
    pub(crate) attributes: Vec<Attribute>
    //Dynamically sized, heap allocated vector of heap allocated Info instances blah blah blah
}

//Access flags for methods and classes
pub enum AccessFlags {
    PUBLIC = 0x1,
    PRIVATE = 0x2,
    PROTECTED = 0x4,
    STATIC = 0x8,
    FINAL = 0x10,
    SYNCHRONIZED = 0x20,
    BRIDGE = 0x40,
    VARARGS = 0x80,
    NATIVE = 0x100,
    ABSTRACT = 0x400,
    STRICT = 0x800,
    SYNTHETIC = 0x1000
}

pub struct MethodInfo {
    access_flags: u16,
    name_index: u16,
    descriptor_index: u16,
    attributes_count: u16,
    attributes: Vec<Attribute>
}

pub struct FieldInfo {
    access_flags: u16,
    name_index: u16,
    descriptor_index: u16,
    attributes_count: u16,
    attributes: Vec<Attribute>
}

impl FieldInfo {
    pub fn from_bytes(rdr: &mut Cursor<&Vec<u8>>) -> Self {
        let count: u16;

        FieldInfo {
            access_flags: rdr.read_u16::<BigEndian>().unwrap(),
            name_index: rdr.read_u16::<BigEndian>().unwrap(),
            descriptor_index: rdr.read_u16::<BigEndian>().unwrap(),
            attributes_count: {
                count = rdr.read_u16::<BigEndian>().unwrap();
                count
            },
            attributes: {
                let mut attributes: Vec<Attribute> = Vec::new();
                for i in 0..count {
                    attributes.push(Attribute::from_bytes(rdr));
                }

                attributes
            }
        }
    }
}

//Attributes define information about a method, field, or class. Not every attribute applies to all
//of the aforementioned types, however. The most direct analogy would be they are like Java annotations
//for the JVM, ironically, because Annotations are actually a form of attribute at compile-time.
pub mod attribute {
    use crate::vm::class::attribute::stackmap::StackMapFrame;
    use std::io::Cursor;
    use byteorder::{ReadBytesExt, BigEndian};
    use crate::vm::class::FromBytes;
    use crate::vm::class::constant::{Constant, PoolTag};

    //https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-4.html#jvms-4.7
    pub struct Attribute {
        attribute_name: String,
        info: AttributeItem
    }

    impl Attribute {
        fn from_bytes(rdr: &mut Cursor<Vec<u8>>, constant_pool: &Vec<Constant>) -> Self {
            let attribute_name_index = rdr.read_u16::<BigEndian>().unwrap();
            let length = rdr.read_u32::<BigEndian>().unwrap();

            let attribute_name_constant: &Constant = constant_pool.get(attribute_name_index as usize).unwrap();

            if(attribute_name_constant.tag != PoolTag::Utf8) { //Needs to be a String
                panic!("Attribute name must be resolve to UTF8 in the constant pool!");
            }

            let attribute_name_utf8: Constant::Utf8 = &attribute_name_constant;
            let attribute_name_string: String;

            if let Constant::Utf8(string) = attribute_name_utf8 {
                attribute_name_string = string;
            }

            Attribute {
                attribute_name: "".to_string(),
                info: {}
            }
        }
    }

    pub enum AttributeItem {
        ConstantValue(ConstantValue),
        Code(Code),
        CodeExceptionTable(CodeExceptionTable),
        StackMapFrame(StackMapFrame),
        Exceptions(Exceptions),
        InnerClasses(InnerClasses),
        InnerClassEntry(InnerClassEntry),
        EnclosingMethod(EnclosingMethod),
        Synthetic(Synthetic),
        Signature(Signature),
        SourceFile(SourceFile),
        SourceDebugExtension(SourceDebugExtension),
        LineNumberTable(LineNumberTable),
        LocalVariableTable(LocalVariableTable),
        Deprecated(Deprecated),
        RuntimeVisibleAnnotations(RuntimeVisibleAnnotations),
        Annotation(Annotation)
    }

    pub struct ConstantValue {
        constant_value_index: u16
    }

    pub struct Code { //This contains the actual, runnable bytecodes of a method/<init>
        max_stack: u16,
        max_locals: u16,
        code_length: u32,
        code: Vec<u8>, //Stream of bytes
        exception_table_length: u16,
        exception_table: Vec<CodeExceptionTable>
    }

    pub struct CodeExceptionTable {
        start_pc: u16,
        end_pc: u16,
        handler_pc: u16,
        catch_type: u16
    }

    pub mod stackmap { //TODO: complete this? https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-4.html#jvms-4.7.4
        // pub struct StackMapTable {
        //     attribute_name_index: u16,
        //     attribute_length: u32,
        //     number_of_entries: u16,
        //     stack_map_frame: Vec<StackmapEntry>
        // }

        pub enum StackMapFrame {
            SameFrame,
            SameLocals1StackItemFrame,
            SameLocals1StackItemFrameExtended,
            ChopFrame,
            SameFrameExtended,
            AppendFrame,
            FullFrame
        }

        pub struct SameFrame {

        }
    }

    pub struct Exceptions {
        number_of_exceptions: u16,
        exception_table_index: Vec<u16>
    }

    pub struct InnerClasses {
        number_of_classes: u16,
        classes: Vec<InnerClassEntry>
    }

    struct InnerClassEntry {
        inner_class_info_index: u16,
        outer_class_info_index: u16,
        inner_name_index: u16,
        inner_class_access_flags: u16
    }

    pub struct EnclosingMethod {
        class_index: u16,
        method_index: u16
    }

    pub struct Synthetic {}

    pub struct Signature {
        signature_index: u16
    }

    pub struct SourceFile {
        sourcefile_index: u16
    }

    pub struct SourceDebugExtension {
        debug_extension: Vec<u8>
    }

    pub struct LineNumberTable {
        line_number_table_length: u16,
        line_number_table: Vec<LineNumberTableEntry>
    }

    pub struct LineNumberTableEntry {
        start_pc: u16,
        line_number: u16
    }

    pub struct LocalVariableTable {
        local_variable_table_length: u16,
        local_variable_table: Vec<LocalVariableTableEntry>
    }

    pub struct LocalVariableTableEntry {
        start_pc: u16,
        length: u16,
        name_index: u16,
        descriptor_index: u16,
        index: u16
    }

    // pub struct LocalVariableTypeTable {
    //     local_variable_type_table_length: u16,
    //     local_variable_type_table: Vec<LocalVariableTypeTableEntry>
    // }

    pub struct LocalVariableTypeEntry {
        start_pc: u16,
        length: u16,
        name_index: u16,
        signature_index: u16,
        index: u16
    }

    pub struct Deprecated {}

    pub struct RuntimeVisibleAnnotations {
        num_annotations: u16,
        annotations: Vec<Annotation>
    }

    pub struct Annotation {
        type_index: u16,
        num_element_value_pairs: u16,
        element_value_pairs: Vec<AnnotationElementValuePair>
    }

    pub struct ElementValue {
        tag: u8,
        value: ElementValueUnion
    }

    pub struct ElementValueUnion {
        const_value_index: u16,
    }

    pub struct AnnotationElementValuePair (u16, ElementValue);
}

pub mod constant {
    use std::io::{Cursor, Read};
    use byteorder::{ReadBytesExt, BigEndian};
    use std::any::Any;

    //https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-4.html#jvms-4.4
    #[repr(u8)]
    #[derive(Debug)]
    pub enum PoolTag {
        Utf8 = 1,
        Integer = 3,
        Float = 4,
        Long = 5,
        Double = 6,
        Class = 7,
        String = 8,
        FieldRef = 9,
        MethodRef = 10,
        InterfaceMethodRef = 11,
        NameAndType = 12,
        MethodHandle = 15,
        MethodType = 16,
        InvokeDynamic = 18,
    }

    impl PoolTag {
        fn as_int(&self) -> u8 {
            match self {
                PoolTag::Utf8 => 1,
                PoolTag::Integer => 3,
                PoolTag::Float => 4,
                PoolTag::Long => 5,
                PoolTag::Double => 6,
                PoolTag::Class => 7,
                PoolTag::String => 8,
                PoolTag::FieldRef => 9,
                PoolTag::MethodRef => 10,
                PoolTag::InterfaceMethodRef => 11,
                PoolTag::NameAndType => 12,
                PoolTag::MethodHandle => 15,
                PoolTag::MethodType => 16,
                PoolTag::InvokeDynamic => 18
            }
        }
    }

    impl PartialEq for PoolTag {
        fn eq(&self, other: &Self) -> bool {
            self.as_int() == other.as_int()
        }
    }

    impl From<u8> for PoolTag {
        fn from(you_ate: u8) -> Self {
            match you_ate {
                1  => Self::Utf8,
                3  => Self::Integer,
                4  => Self::Float,
                5  => Self::Long,
                6  => Self::Double,
                7  => Self::Class,
                8  => Self::String,
                9  => Self::FieldRef,
                10 => Self::MethodRef,
                11 => Self::InterfaceMethodRef,
                12 => Self::NameAndType,
                15 => Self::MethodHandle,
                16 => Self::MethodType,
                18 => Self::InvokeDynamic,

                tag => panic!("invalid pool tag: {}", tag),
            }
        }
    }

    pub enum Constant {
        ///bytes
        Utf8(String),
        Integer(u32),
        Float(u32),
        Long(u64),
        Double(u64),
        ///name_index
        Class(u16),
        ///string_index
        String(u16),
        ///class_index, name_and_type_index
        FieldRef(u16,u16),
        ///class_index, name_and_type_index
        MethodRef(u16, u16),
        ///class_index, name_and_type_index
        InterfaceMethodRef(u16, u16),
        ///name_index, descriptor_index
        NameAndType(u16,u16),
        ///reference_king, reference_index
        MethodHandle(u8,u16),
        ///descriptor_index
        MethodType(u16),
        ///bootstrap_method_attr_index, name_and_type_index
        InvokeDynamic(u16, u16)
    }

    impl Constant {
        pub fn from_bytes(rdr: &mut Cursor<&Vec<u8>>) -> Constant {
            let tag = rdr.read_u8().unwrap();
            let as_pool_tag: PoolTag = tag.into();

            match as_pool_tag {
                PoolTag::Utf8 => {
                    let length = rdr.read_u16::<BigEndian>().unwrap();
                    let mut buf = Vec::new();
                    for i in 0..length {
                        buf.push(rdr.read_u8().unwrap());
                    }
                    Constant::Utf8(String::from_utf8(buf).unwrap())
                },
                PoolTag::Integer => Constant::Integer(rdr.read_u32::<BigEndian>().unwrap()),
                PoolTag::Float => Constant::Float(rdr.read_u32::<BigEndian>().unwrap()),
                PoolTag::Long => Constant::Long(
                        (rdr.read_u32::<BigEndian>().unwrap() as u64) << 32 & (rdr.read_u32::<BigEndian>().unwrap() as u64)
                    ),
                PoolTag::Double => Constant::Long(
                    (rdr.read_u32::<BigEndian>().unwrap() as u64) << 32 & (rdr.read_u32::<BigEndian>().unwrap() as u64)
                ),
                PoolTag::Class => Constant::Class(rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::String => Constant::String(rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::FieldRef => Constant::FieldRef(rdr.read_u16::<BigEndian>().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::MethodRef => Constant::MethodRef(rdr.read_u16::<BigEndian>().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::InterfaceMethodRef => Constant::InterfaceMethodRef(rdr.read_u16::<BigEndian>().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::NameAndType => Constant::NameAndType(rdr.read_u16::<BigEndian>().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::MethodHandle => Constant::MethodHandle(rdr.read_u8().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::MethodType => Constant::MethodType(rdr.read_u16::<BigEndian>().unwrap()),
                PoolTag::InvokeDynamic => Constant::InvokeDynamic(rdr.read_u16::<BigEndian>().unwrap(), rdr.read_u16::<BigEndian>().unwrap()),
                e => panic!("Unknown constant tag type {}!",e.as_int())
            }
        }
    }
}
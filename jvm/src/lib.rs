#![feature(seek_convenience)]
#![feature(try_trait)]

pub mod vm;

#[cfg(test)]
mod tests {
    use crate::vm::class::FieldDescriptor;

    #[test]
    fn assert_field_descriptor_parse() {
        let a = FieldDescriptor::parse("Ljava/lang/Object;");
        let b = FieldDescriptor::parse("[[[[[B");

        if let FieldDescriptor::ObjectType(string) = a {
            assert_eq!("java/lang/Object", string);
        } else {  panic!("FieldDescriptor must be an ObjectType variant"); }

        if let FieldDescriptor::ArrayType(array_type) = b {
            let f = *array_type.field_descriptor;
            if let FieldDescriptor::BaseType(base_type) = f {
            } else { panic!("FieldDescriptor must be a BaseType variant!"); }
        } else { panic!("FieldDescriptor must be an ArrayType variant"); }
    }
}
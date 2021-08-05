import java.lang.String;

public class Main {

    public static int static_thing = Test.TEST_STATIC;

    public int integer = 0;

    public static native void print_int(int i);
//
    public static native void print_string(String str);

    public static native void panic();

    public void print() {
        print_int(this.integer);
    }

    public static void main(String[] strings) {
        //Cast test, this should work
        String string = "Hello world!";
        Object string_object = (Object) string;
        String string_from_cast = (String) string_object;
        //Fail
        Main fail_cast = (Main) string_object;
    }

    public static void main1(String[] str) {
    }

}
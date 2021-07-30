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

//
//    public static native long get_time();
//
//    public static native void print_long(long l);

    public static void main(String[] strings) {
        print_string("i løve møøse");
//
        char[] chars = new char[2];
        chars[0] = 'h';
        chars[1] = 'i';
        String string = new String(chars);

        print_string(string);

        print_string(strings[0]);
    }

    public static void main1(String[] str) {
    }

}
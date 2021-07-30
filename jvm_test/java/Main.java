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
        print_int(static_thing);
    }

    public static void main1(String[] str) {
    }

}
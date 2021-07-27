import java.lang.String;

public class Main {

    public static native void print_int(int i);
//
    public static native void print_string(String i);
//
//    public static native long get_time();
//
//    public static native void print_long(long l);

    public static void main(String[] str) {
        while(true) {
            print_int(0);
        }
    }

    public static void main1(String[] str) {
        while(true) {
            print_int(1);
        }
    }

}
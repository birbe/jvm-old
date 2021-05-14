import java.lang.String;

public class Main {

//    public static native void print_int(int i);
//
    public static native void print_string(String string);
//
//    public static native long get_time();
//
//    public static native void print_long(long l);

    public static void main(int i) {
        for(int a=0;a<10;a++) {
            print_string("hi");
        }
    }

}
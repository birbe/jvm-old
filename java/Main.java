import java.lang.String;

public class Main {

    static {
        print_string("Something, lol idk");
    }

    public Main() {

    }

    public static native void print_int(int a);

    public static native void print_string(String str);

    public static native long get_time();

    public SomeInterface returns_interface() {
        return null;
    }

    public static void main(String[] args) {
        //Checking time to allocate 10000 objects
//        long time = get_time();
        print_string("Allocating 1 million objects...");
        for(int i=0;i<1000000;i++) {
            Object obj = new Object();
        }
        print_string("Done.");
    }
}
import java.lang.String;

public class Main {

    public static String STRING;
    public static int INT = 5;

    public int thingy = 0;

    public static native void print_int(int i);
//
    public static native void print_string(int i);

    public static native void panic();

    public void print() {
        print_int(this.thingy);
    }

//
//    public static native long get_time();
//
//    public static native void print_long(long l);

    public static void main(String[] str) {
        while(true) {
            Object obj = new Object();
            obj.field = 7;
            if(obj.field != 7) {
                print_int(obj.field);
                panic();
            }
        }
    }

    public static void main1(String[] str) {
//        while(true) {
//            Object obj = new Object();
//            obj.field = 255;
//        }
    }

}
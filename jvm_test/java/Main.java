import java.lang.String;

public class Main {

//    public static native void print_int(char[] i);
//
    public static native void print_string(String string);
//
//    public static native long get_time();
//
//    public static native void print_long(long l);

    public static void main(String[] args) {
        int i = 0;
        switch(i) {
            case 0:
                print_string("Zero");
                if(i == 1) {
                    print_string("Inside if");
                } else {
                    print_string("Else");
                }
                print_string("Anyhow");
            default:
                print_string("Default");
        }
    }

}
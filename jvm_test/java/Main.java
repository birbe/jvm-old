import java.lang.String;

public class Main {

    public static native void print_int(int i);

    public static native void print_string(String string);

    public static native long get_time();

    public static native void print_long(long l);

    public static void main(String[] args) {
        char[] array = new char[5];
        array[0] = 'h';
        array[1] = 'e';
        array[2] = 'l';
        array[3] = 'l';
        array[4] = 'o';

        String string = new String(array);
        print_string(string);
    }

}
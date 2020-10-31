import java.lang.String;
import birb.Event;

import birb.MessageEventSubclass;

public class Main {

    public Main() {

    }

    public static native void print_int(int a);

    public static native void print_string(String str);

    public static native void print_benchmark();

    public String idk() {
        return "FUCK";
    }

    public static void main(String[] args) {
        print_string("Hello world!");
    }
}
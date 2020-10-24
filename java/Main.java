public class Main {

    public Main() {

    }

    public static native void print_int(int a);

    public static native void print_string(String str);

    public SomeInterface returns_interface() {
        return null;
    }

    public static void main(String[] args) {
        Main.print_string("Let's make it invoke java/lang/String");
    }
}
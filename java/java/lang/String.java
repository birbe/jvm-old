package java.lang;

public class String {

    static {
        print("Test");
    }

    private char[] chars;

    public String(char[] chars) {
        this.chars = chars;
    }

    public static native void print(String s);
}
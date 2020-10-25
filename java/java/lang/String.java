package java.lang;

public class String {

    private char[] chars;

    public String(char[] chars) {
        this.chars = chars;
    }

    public static native void print(String s);
}
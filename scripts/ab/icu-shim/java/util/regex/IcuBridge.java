package java.util.regex;

final class IcuBridge {
    static {
        String path = System.getProperty("legado.icu.bridge");
        if (path != null && !path.isEmpty()) {
            System.load(path);
        } else {
            System.loadLibrary("icu_bridge");
        }
    }

    private IcuBridge() {}

    static native long compile(String pattern, int flags);
    static native void close(long handle);
    static native void setText(long handle, String text);
    static native boolean find(long handle);
    static native boolean findFrom(long handle, int start);
    static native int groupCount(long handle);
    static native String group(long handle, int idx);
    static native int start(long handle, int idx);
    static native int end(long handle, int idx);
    static native String replaceAll(long handle, String replacement);
}

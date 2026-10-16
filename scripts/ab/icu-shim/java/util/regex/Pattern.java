package java.util.regex;

public final class Pattern implements java.io.Serializable {

    public static final int UNIX_LINES = 0x01;
    public static final int CASE_INSENSITIVE = 0x02;
    public static final int COMMENTS = 0x04;
    public static final int MULTILINE = 0x08;
    public static final int LITERAL = 0x10;
    public static final int DOTALL = 0x20;
    public static final int UNICODE_CASE = 0x40;
    public static final int CANON_EQ = 0x80;
    public static final int UNICODE_CHARACTER_CLASS = 0x100;

    private static final int ICU_FLAGS =
            UNIX_LINES | CASE_INSENSITIVE | COMMENTS | MULTILINE | LITERAL | DOTALL;

    private final String regex;
    private final int flags;
    private long handle;

    private Pattern(String p, int f) {
        if ((f & CANON_EQ) != 0) {
            throw new IllegalArgumentException("CANON_EQ flag isn't supported");
        }
        if ((f & UNICODE_CHARACTER_CLASS) != 0) {
            throw new IllegalArgumentException("UNICODE_CHARACTER_CLASS flag not supported");
        }
        this.regex = p;
        this.flags = f;
    }

    public static Pattern compile(String regex) {
        return new Pattern(regex, 0);
    }

    public static Pattern compile(String regex, int flags) {
        return new Pattern(regex, flags);
    }

    public Matcher matcher(CharSequence input) {
        return new Matcher(this, input);
    }

    public String pattern() {
        return regex;
    }

    public int flags() {
        return flags;
    }

    public static String quote(String s) {
        int len = s.length();
        StringBuilder sb = new StringBuilder(len * 2 + 6);
        sb.append("\\Q");
        int slashE = 0;
        for (int i = 0; i < len; i++) {
            char c = s.charAt(i);
            if (c == '\\' || c == 'E') {
                sb.append(s, slashE, i).append("\\E");
                if (c == '\\') {
                    sb.append("\\\\");
                } else {
                    sb.append("\\E");
                }
                slashE = i + 1;
            }
        }
        return sb.append(s, slashE, len).append("\\E").toString();
    }

    public String[] split(CharSequence input) {
        return split(input, 0);
    }

    public String[] split(CharSequence input, int limit) {
        int index = 0;
        boolean matchLimited = limit > 0;
        java.util.ArrayList<String> matchList = new java.util.ArrayList<>();
        Matcher m = matcher(input);
        while (m.find()) {
            if (!matchLimited || matchList.size() < limit - 1) {
                if (index == 0 && index == m.start() && m.start() == m.end()) {
                    continue;
                }
                matchList.add(input.subSequence(index, m.start()).toString());
                index = m.end();
            } else if (matchList.size() == limit - 1) {
                matchList.add(input.subSequence(index, input.length()).toString());
                index = m.end();
            }
        }
        if (index == 0) {
            return new String[]{input.toString()};
        }
        if (!matchLimited || matchList.size() < limit) {
            matchList.add(input.subSequence(index, input.length()).toString());
        }
        int resultSize = matchList.size();
        if (limit == 0) {
            while (resultSize > 0 && matchList.get(resultSize - 1).isEmpty()) {
                resultSize--;
            }
        }
        String[] result = new String[resultSize];
        return matchList.subList(0, resultSize).toArray(result);
    }

    long handle() {
        if (handle == 0) {
            handle = IcuBridge.compile(regex, flags & ICU_FLAGS);
        }
        return handle;
    }

    @Override
    public String toString() {
        return regex;
    }
}

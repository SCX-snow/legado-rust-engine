package java.util.regex;

public final class Matcher implements MatchResult {

    private final Pattern parent;
    private final CharSequence text;
    private boolean textSet;
    private boolean matched;
    private int appendPos;
    private int lastStart = -1;
    private int lastEnd = -1;
    private int regionStart = 0;
    private int regionEnd = Integer.MAX_VALUE;

    Matcher(Pattern parent, CharSequence input) {
        this.parent = parent;
        this.text = input == null ? "" : input;
    }

    public Pattern pattern() {
        return parent;
    }

    private void ensureText() {
        if (!textSet) {
            IcuBridge.setText(parent.handle(), text.toString());
            textSet = true;
        }
    }

    public Matcher reset() {
        textSet = false;
        matched = false;
        appendPos = 0;
        lastStart = -1;
        lastEnd = -1;
        return this;
    }

    public boolean find() {
        ensureText();
        matched = IcuBridge.find(parent.handle());
        if (matched) {
            lastStart = IcuBridge.start(parent.handle(), 0);
            lastEnd = IcuBridge.end(parent.handle(), 0);
        }
        return matched;
    }

    public boolean find(int start) {
        ensureText();
        matched = IcuBridge.findFrom(parent.handle(), start);
        if (matched) {
            lastStart = IcuBridge.start(parent.handle(), 0);
            lastEnd = IcuBridge.end(parent.handle(), 0);
        }
        return matched;
    }

    public Matcher region(int start, int end) {
        regionStart = start;
        regionEnd = end;
        return this;
    }

    public int regionStart() {
        return regionStart;
    }

    public int regionEnd() {
        return regionEnd;
    }

    public Matcher useAnchoringBounds(boolean b) {
        return this;
    }

    public boolean hasAnchoringBounds() {
        return true;
    }

    public Matcher useTransparentBounds(boolean b) {
        return this;
    }

    public boolean hasTransparentBounds() {
        return false;
    }

    public boolean hitEnd() {
        return lastEnd >= text.length();
    }

    public boolean requireEnd() {
        return false;
    }

    public MatchResult toMatchResult() {
        return this;
    }

    public static String quoteReplacement(String s) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '\\' || c == '$') sb.append('\\');
            sb.append(c);
        }
        return sb.toString();
    }

    public boolean matches() {
        reset();
        if (!find()) return false;
        return lastStart == 0 && lastEnd == text.length();
    }

    public boolean lookingAt() {
        reset();
        if (!find()) return false;
        return lastStart == 0;
    }

    private void requireMatch() {
        if (!matched) throw new IllegalStateException("No match found");
    }

    @Override
    public int start() {
        requireMatch();
        return lastStart;
    }

    @Override
    public int start(int group) {
        requireMatch();
        return IcuBridge.start(parent.handle(), group);
    }

    @Override
    public int end() {
        requireMatch();
        return lastEnd;
    }

    @Override
    public int end(int group) {
        requireMatch();
        return IcuBridge.end(parent.handle(), group);
    }

    @Override
    public String group() {
        requireMatch();
        return IcuBridge.group(parent.handle(), 0);
    }

    @Override
    public String group(int group) {
        requireMatch();
        if (IcuBridge.start(parent.handle(), group) < 0) return null;
        return IcuBridge.group(parent.handle(), group);
    }

    @Override
    public int groupCount() {
        ensureText();
        return IcuBridge.groupCount(parent.handle());
    }

    public int end(String name) {
        throw new UnsupportedOperationException("named groups not supported by harness shim");
    }

    public String group(String name) {
        throw new UnsupportedOperationException("named groups not supported by harness shim");
    }

    public Matcher appendReplacement(StringBuffer sb, String replacement) {
        requireMatch();
        sb.append(text, appendPos, lastStart);
        StringBuilder out = new StringBuilder();
        int i = 0;
        while (i < replacement.length()) {
            char c = replacement.charAt(i++);
            if (c == '\\') {
                if (i < replacement.length()) {
                    out.append(replacement.charAt(i++));
                } else {
                    out.append('\\');
                }
            } else if (c == '$') {
                if (i < replacement.length() && replacement.charAt(i) == '{') {
                    int j = replacement.indexOf('}', i);
                    if (j < 0) throw new IllegalArgumentException("Named capture group is missing trailing '}'");
                    String name = replacement.substring(i + 1, j);
                    out.append(group(name));
                    i = j + 1;
                } else {
                    int ref = -1;
                    int start = i;
                    while (i < replacement.length() && Character.isDigit(replacement.charAt(i))) {
                        ref = (ref < 0 ? 0 : ref * 10) + (replacement.charAt(i) - '0');
                        i++;
                    }
                    if (ref < 0) throw new IllegalArgumentException("Illegal group reference");
                    if (ref > groupCount()) {
                        out.append('$').append(replacement, start, i);
                    } else {
                        String g = group(ref);
                        out.append(g == null ? "" : g);
                    }
                }
            } else {
                out.append(c);
            }
        }
        sb.append(out);
        appendPos = lastEnd;
        return this;
    }

    public StringBuffer appendTail(StringBuffer sb) {
        sb.append(text, appendPos, text.length());
        return sb;
    }

    public String replaceAll(String replacement) {
        reset();
        StringBuffer sb = new StringBuffer();
        while (find()) {
            appendReplacement(sb, replacement);
        }
        return appendTail(sb).toString();
    }

    public String replaceFirst(String replacement) {
        reset();
        StringBuffer sb = new StringBuffer();
        if (find()) {
            appendReplacement(sb, replacement);
        }
        return appendTail(sb).toString();
    }

    @Override
    public String toString() {
        return "Matcher[" + parent.pattern() + "]";
    }
}

#include <jni.h>
#include <stdlib.h>
#include <string.h>
#include <unicode/uregex.h>
#include <unicode/ustring.h>

typedef struct {
    URegularExpression *re;
    UChar *text;
    int32_t tlen;
} Bridge;

static UChar *to_u16(JNIEnv *env, jstring s, int32_t *out_len) {
    if (s == NULL) { *out_len = 0; return NULL; }
    const jchar *chars = (*env)->GetStringChars(env, s, NULL);
    jsize n = (*env)->GetStringLength(env, s);
    UChar *buf = (UChar *)malloc((n + 1) * sizeof(UChar));
    if (buf == NULL) { (*env)->ReleaseStringChars(env, s, chars); return NULL; }
    memcpy(buf, chars, (size_t)n * sizeof(UChar));
    buf[n] = 0;
    (*env)->ReleaseStringChars(env, s, chars);
    *out_len = (int32_t)n;
    return buf;
}

static jstring to_jstring(JNIEnv *env, const UChar *s, int32_t len) {
    if (s == NULL) return NULL;
    return (*env)->NewString(env, (const jchar *)s, (jsize)len);
}

JNIEXPORT jlong JNICALL Java_java_util_regex_IcuBridge_compile
  (JNIEnv *env, jclass cls, jstring pattern, jint flags) {
    (void)cls;
    int32_t plen = 0;
    UChar *pat = to_u16(env, pattern, &plen);
    if (pat == NULL && plen != 0) return 0;
    UErrorCode st = U_ZERO_ERROR;
    URegularExpression *re = uregex_open(pat, plen, (uint32_t)flags, NULL, &st);
    free(pat);
    if (U_FAILURE(st)) {
        jclass rex = (*env)->FindClass(env, "java/util/regex/PatternSyntaxException");
        if (rex != NULL) {
            (*env)->ThrowNew(env, rex, u_errorName(st));
        }
        return 0;
    }
    Bridge *b = (Bridge *)calloc(1, sizeof(Bridge));
    b->re = re;
    return (jlong)(intptr_t)b;
}

JNIEXPORT void JNICALL Java_java_util_regex_IcuBridge_close
  (JNIEnv *env, jclass cls, jlong handle) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return;
    if (b->re != NULL) uregex_close(b->re);
    free(b->text);
    free(b);
}

JNIEXPORT void JNICALL Java_java_util_regex_IcuBridge_setText
  (JNIEnv *env, jclass cls, jlong handle, jstring text) {
    (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return;
    int32_t len = 0;
    UChar *buf = to_u16(env, text, &len);
    free(b->text);
    b->text = buf;
    b->tlen = len;
    UErrorCode st = U_ZERO_ERROR;
    uregex_setText(b->re, b->text, b->tlen, &st);
}

JNIEXPORT jboolean JNICALL Java_java_util_regex_IcuBridge_find
  (JNIEnv *env, jclass cls, jlong handle) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return JNI_FALSE;
    UErrorCode st = U_ZERO_ERROR;
    UBool ok = uregex_findNext(b->re, &st);
    return U_SUCCESS(st) && ok ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL Java_java_util_regex_IcuBridge_findFrom
  (JNIEnv *env, jclass cls, jlong handle, jint start) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return JNI_FALSE;
    UErrorCode st = U_ZERO_ERROR;
    UBool ok = uregex_find(b->re, start, &st);
    return U_SUCCESS(st) && ok ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jint JNICALL Java_java_util_regex_IcuBridge_groupCount
  (JNIEnv *env, jclass cls, jlong handle) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return 0;
    UErrorCode st = U_ZERO_ERROR;
    int32_t n = uregex_groupCount(b->re, &st);
    return U_SUCCESS(st) ? (jint)n : 0;
}

JNIEXPORT jstring JNICALL Java_java_util_regex_IcuBridge_group
  (JNIEnv *env, jclass cls, jlong handle, jint idx) {
    (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return NULL;
    UErrorCode st = U_ZERO_ERROR;
    int32_t need = uregex_group(b->re, idx, NULL, 0, &st);
    if (st != U_BUFFER_OVERFLOW_ERROR && U_FAILURE(st)) return NULL;
    st = U_ZERO_ERROR;
    UChar *buf = (UChar *)malloc((need + 1) * sizeof(UChar));
    if (buf == NULL) return NULL;
    uregex_group(b->re, idx, buf, need + 1, &st);
    if (U_FAILURE(st)) { free(buf); return NULL; }
    jstring out = to_jstring(env, buf, need);
    free(buf);
    return out;
}

JNIEXPORT jint JNICALL Java_java_util_regex_IcuBridge_start
  (JNIEnv *env, jclass cls, jlong handle, jint idx) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return -1;
    UErrorCode st = U_ZERO_ERROR;
    int32_t v = uregex_start(b->re, idx, &st);
    return U_SUCCESS(st) ? (jint)v : -1;
}

JNIEXPORT jint JNICALL Java_java_util_regex_IcuBridge_end
  (JNIEnv *env, jclass cls, jlong handle, jint idx) {
    (void)env; (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return -1;
    UErrorCode st = U_ZERO_ERROR;
    int32_t v = uregex_end(b->re, idx, &st);
    return U_SUCCESS(st) ? (jint)v : -1;
}

JNIEXPORT jstring JNICALL Java_java_util_regex_IcuBridge_replaceAll
  (JNIEnv *env, jclass cls, jlong handle, jstring replacement) {
    (void)cls;
    Bridge *b = (Bridge *)(intptr_t)handle;
    if (b == NULL) return NULL;
    int32_t rlen = 0;
    UChar *rep = to_u16(env, replacement, &rlen);
    UErrorCode st = U_ZERO_ERROR;
    int32_t cap = b->tlen * 4 + 256;
    UChar *out = (UChar *)malloc((size_t)cap * sizeof(UChar));
    int32_t olen = uregex_replaceAll(b->re, rep, rlen, out, cap, &st);
    free(rep);
    if (U_FAILURE(st)) { free(out); return NULL; }
    jstring js = to_jstring(env, out, olen);
    free(out);
    return js;
}

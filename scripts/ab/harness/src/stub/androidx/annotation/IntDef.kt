package androidx.annotation

@Retention(AnnotationRetention.SOURCE)
@Target(AnnotationTarget.ANNOTATION_CLASS, AnnotationTarget.VALUE_PARAMETER, AnnotationTarget.FIELD)
annotation class IntDef(vararg val value: Int, val flag: Boolean = false)

@Retention(AnnotationRetention.SOURCE)
@Target(AnnotationTarget.ANNOTATION_CLASS, AnnotationTarget.VALUE_PARAMETER, AnnotationTarget.FIELD)
annotation class StringDef(vararg val value: String, val flag: Boolean = false)

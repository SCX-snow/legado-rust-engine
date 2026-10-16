package android.annotation

@Retention(AnnotationRetention.SOURCE)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION, AnnotationTarget.PROPERTY,
    AnnotationTarget.FIELD, AnnotationTarget.CONSTRUCTOR, AnnotationTarget.VALUE_PARAMETER)
annotation class SuppressLint(vararg val value: String)

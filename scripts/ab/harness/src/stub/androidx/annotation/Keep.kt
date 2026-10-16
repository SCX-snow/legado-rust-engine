package androidx.annotation

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION, AnnotationTarget.PROPERTY,
    AnnotationTarget.FIELD, AnnotationTarget.CONSTRUCTOR)
annotation class Keep

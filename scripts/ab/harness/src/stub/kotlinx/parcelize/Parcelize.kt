package kotlinx.parcelize

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS)
annotation class Parcelize

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.FIELD, AnnotationTarget.PROPERTY)
annotation class IgnoredOnParcel

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.FIELD, AnnotationTarget.PROPERTY)
annotation class RawValue

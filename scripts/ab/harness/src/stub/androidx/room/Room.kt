package androidx.room

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS)
annotation class Entity(
    val tableName: String = "",
    val indices: Array<Index> = [],
    val primaryKeys: Array<String> = [],
    val foreignKeys: Array<ForeignKey> = [],
    val inheritSuperIndices: Boolean = false,
)

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS)
annotation class Index(
    val value: Array<String>,
    val unique: Boolean = false,
)

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS)
annotation class ForeignKey(
    val entity: kotlin.reflect.KClass<*>,
    val parentColumns: Array<String>,
    val childColumns: Array<String>,
    val onDelete: Int = NO_ACTION,
) {
    companion object {
        const val NO_ACTION = 1
        const val RESTRICT = 2
        const val SET_NULL = 3
        const val SET_DEFAULT = 4
        const val CASCADE = 5
    }
}

@Retention(AnnotationRetention.BINARY)
@Target(
    AnnotationTarget.FIELD, AnnotationTarget.FUNCTION, AnnotationTarget.VALUE_PARAMETER,
    AnnotationTarget.PROPERTY_GETTER,
)
annotation class PrimaryKey(val autoGenerate: Boolean = false)

@Retention(AnnotationRetention.BINARY)
@Target(
    AnnotationTarget.FIELD, AnnotationTarget.FUNCTION, AnnotationTarget.VALUE_PARAMETER,
    AnnotationTarget.PROPERTY_GETTER,
)
annotation class ColumnInfo(
    val name: String = "",
    val defaultValue: String = VALUE_UNSPECIFIED,
    val typeAffinity: Int = UNDEFINED,
) {
    companion object {
        const val VALUE_UNSPECIFIED = "[value-unspecified]"
        const val UNDEFINED = 1
    }
}

@Retention(AnnotationRetention.BINARY)
@Target(
    AnnotationTarget.FIELD, AnnotationTarget.FUNCTION, AnnotationTarget.CONSTRUCTOR,
    AnnotationTarget.PROPERTY_GETTER,
)
annotation class Ignore

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION, AnnotationTarget.FIELD)
annotation class TypeConverter

@Retention(AnnotationRetention.BINARY)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FILE)
annotation class TypeConverters(vararg val value: kotlin.reflect.KClass<*>)

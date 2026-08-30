# kotlinx.serialization keeps its generated serializers via @Serializable; R8
# needs the companion serializer members preserved for the JSON Feed models.
-keepclassmembers class ** {
    *** Companion;
}
-keepclasseswithmembers class ** {
    kotlinx.serialization.KSerializer serializer(...);
}

# OkHttp ships optional Conscrypt/BouncyCastle hooks that R8 warns about.
-dontwarn okhttp3.internal.platform.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**

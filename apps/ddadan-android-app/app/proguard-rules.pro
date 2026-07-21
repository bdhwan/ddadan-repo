# ── kotlinx.serialization ───────────────────────────────────────────────
# @Serializable 모델과 자동 생성된 $$serializer 를 보존한다.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**

-keep,includedescriptorclasses class com.ddadan.player.**$$serializer { *; }
-keepclassmembers class com.ddadan.player.** {
    *** Companion;
}
-keepclasseswithmembers class com.ddadan.player.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# ── Retrofit ────────────────────────────────────────────────────────────
-keepattributes Signature, RuntimeVisibleAnnotations, RuntimeVisibleParameterAnnotations, AnnotationDefault
-keepclassmembers,allowshrinking,allowobfuscation interface * {
    @retrofit2.http.* <methods>;
}
-keep,allowobfuscation,allowshrinking class retrofit2.Response
-keep,allowobfuscation,allowshrinking class kotlin.coroutines.Continuation

# ── 우리 API 인터페이스/모델 (리플렉션 경로 보호) ─────────────────────────
-keep interface com.ddadan.player.data.PlayerApi { *; }

# OkHttp / Okio / Coil / Media3 는 각 라이브러리의 consumer rules 로 처리됨.

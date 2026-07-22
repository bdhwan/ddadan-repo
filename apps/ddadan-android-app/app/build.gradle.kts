plugins {
  alias(libs.plugins.android.application)
  alias(libs.plugins.compose.compiler)
  alias(libs.plugins.kotlin.serialization)
}

// Server the app talks to. Set in gradle.properties, overridable per build with
// -PDDADAN_API_BASE=... . Debug reads it too: a debug build is the only one that
// installs without a signing config, so it is what goes on a real device, and
// hardcoding the emulator loopback there left no way to point one at a server.
val ddadanApiBase = (project.findProperty("DDADAN_API_BASE") as String?)
  ?: "http://10.0.2.2:7800/api"

android {
  namespace = "com.ddadan.player"
  compileSdk = 36
  defaultConfig {
    applicationId = "com.ddadan.player"
    minSdk = 22
    targetSdk = 36
    versionCode = 21
    versionName = "1.5"
    buildConfigField("String", "API_BASE", "\"$ddadanApiBase\"")
    buildConfigField("long", "POLL_INTERVAL_MS", "5000L")
  }

  signingConfigs {
    // 사내 사이니지 앱: OTA 서명 일관성을 위해 debug 키스토어로 release도 서명한다.
    // (이미 배포된 debug-서명 앱을 pm install -r로 갱신하려면 같은 키여야 함)
    create("release") {
      storeFile = file(System.getProperty("user.home") + "/.android/debug.keystore")
      storePassword = "android"
      keyAlias = "androiddebugkey"
      keyPassword = "android"
    }
  }

  buildTypes {
    debug {
      buildConfigField("String", "API_BASE", "\"$ddadanApiBase\"")
    }
    release {
      isMinifyEnabled = true
      isShrinkResources = true
      signingConfig = signingConfigs.getByName("release")
      proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
      buildConfigField("String", "API_BASE", "\"$ddadanApiBase\"")
    }
  }
  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  buildFeatures {
    compose = true
    buildConfig = true
    aidl = false
    shaders = false
  }

  packaging {
    resources {
      excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
  }
}

kotlin {
  jvmToolchain(17)
}

dependencies {
  implementation(project(":core"))

  val composeBom = platform(libs.androidx.compose.bom)
  implementation(composeBom)
  androidTestImplementation(composeBom)

  implementation(libs.androidx.core.ktx)
  implementation(libs.androidx.lifecycle.runtime.ktx)
  implementation(libs.androidx.activity.compose)
  implementation(libs.androidx.lifecycle.runtime.compose)
  implementation(libs.androidx.lifecycle.viewmodel.compose)
  implementation(libs.androidx.compose.ui)
  implementation(libs.androidx.compose.ui.tooling.preview)
  implementation(libs.androidx.compose.material3)
  implementation(libs.androidx.datastore.preferences)
  implementation(libs.androidx.media3.exoplayer)
  implementation(libs.androidx.media3.ui)
  implementation(libs.coil.compose)
  implementation(libs.coil.network.okhttp)
  implementation(libs.kotlinx.serialization.json)
  implementation(libs.okhttp.logging)
  implementation(libs.retrofit)
  implementation(libs.retrofit.kotlinx.serialization)

  debugImplementation(libs.androidx.compose.ui.tooling)
  androidTestImplementation(libs.androidx.compose.ui.test.junit4)
  debugImplementation(libs.androidx.compose.ui.test.manifest)

  testImplementation(libs.junit)
  testImplementation(libs.kotlinx.coroutines.test)

  androidTestImplementation(libs.androidx.test.core)
  androidTestImplementation(libs.androidx.test.ext.junit)
  androidTestImplementation(libs.androidx.test.runner)
  androidTestImplementation(libs.androidx.test.espresso.core)
}

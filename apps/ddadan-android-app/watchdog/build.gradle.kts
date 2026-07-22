plugins {
  alias(libs.plugins.android.application)
}

android {
  namespace = "com.ddadan.watchdog"
  compileSdk = 36
  defaultConfig {
    applicationId = "com.ddadan.watchdog"
    minSdk = 22
    targetSdk = 36
    versionCode = 7
    versionName = "7.0"
  }

  signingConfigs {
    // 플레이어와 동일 debug 키스토어로 release 서명(root pm install -r 서명 호환).
    create("release") {
      storeFile = file(System.getProperty("user.home") + "/.android/debug.keystore")
      storePassword = "android"
      keyAlias = "androiddebugkey"
      keyPassword = "android"
    }
  }

  buildTypes {
    release {
      isMinifyEnabled = true
      isShrinkResources = true
      signingConfig = signingConfigs.getByName("release")
      proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
    }
  }
  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  buildFeatures {
    buildConfig = true
  }
}

kotlin {
  jvmToolchain(17)
}

dependencies {
  implementation(project(":core"))
  implementation(libs.androidx.core.ktx)
  implementation(libs.kotlinx.coroutines.android)
}

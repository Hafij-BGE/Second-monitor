plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.hafijbge.secondmonitor"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.github.hafijbge.secondmonitor"
        minSdk = 24
        targetSdk = 35
        versionCode = (System.getenv("SMON_VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("SMON_VERSION_NAME") ?: "0.1.0"
    }

    signingConfigs {
        // CI passes a fixed key so new APKs install over old ones.
        // Without it, release builds fall back to the debug key.
        val keystore = System.getenv("SMON_KEYSTORE_FILE")
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("SMON_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SMON_KEY_ALIAS")
                keyPassword = System.getenv("SMON_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

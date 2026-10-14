plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "io.github.hafijbge.secondmonitor"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.github.hafijbge.extendo"
        minSdk = 24
        targetSdk = 35
        versionCode = (System.getenv("SMON_VERSION_CODE") ?: "2").toInt()
        versionName = System.getenv("SMON_VERSION_NAME") ?: "0.2.0"
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
            // R8 strips the unused Material icons, keeping the APK small.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
}

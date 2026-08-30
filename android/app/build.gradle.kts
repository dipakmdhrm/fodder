plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
    alias(libs.plugins.ktlint)
    jacoco
}

// Coverage for the JVM unit tests (report-only; CI uploads it as an artifact).
// The jacoco plugin instruments testDebugUnitTest automatically; this task just
// renders the .exec data into XML/HTML.
tasks.register<JacocoReport>("jacocoDebugUnitTestReport") {
    dependsOn("testDebugUnitTest")
    reports {
        xml.required = true
        html.required = true
    }
    classDirectories.setFrom(
        fileTree(layout.buildDirectory.dir("tmp/kotlin-classes/debug")) {
            exclude(
                "**/R.class",
                "**/R$*.class",
                "**/BuildConfig.*",
                "**/Manifest*.*",
                // Compose UI and generated Room code are outside the JVM-test boundary.
                "**/ui/**",
                "**/*_Impl*.class",
            )
        },
    )
    sourceDirectories.setFrom(files("src/main/java"))
    executionData.setFrom(layout.buildDirectory.files("jacoco/testDebugUnitTest.exec"))
}

android {
    namespace = "io.github.dipakmdhrm.fodder"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.github.dipakmdhrm.fodder"
        minSdk = 26
        targetSdk = 35

        // The tag is the single source of version truth: release-android.yml
        // derives both of these from `android-X.Y.Z` and passes them in. A local
        // build is always "dev"/1, which no release can collide with.
        versionCode = System.getenv("VERSION_CODE")?.toIntOrNull() ?: 1
        versionName = System.getenv("VERSION_NAME") ?: "dev"
    }

    signingConfigs {
        create("release") {
            // Populated by CI from repository secrets. The keystore is decoded
            // next to this module at build time and never committed.
            val keystore = file("../fodder.keystore")
            if (keystore.exists()) {
                storeFile = keystore
                storePassword = System.getenv("KEYSTORE_PASSWORD")
                keyAlias = System.getenv("KEY_ALIAS")
                keyPassword = System.getenv("KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            // Falls back to the debug signing config locally, where no keystore
            // is present, so `assembleRelease` still works for a smoke test.
            signingConfig =
                if (file("../fodder.keystore").exists()) {
                    signingConfigs.getByName("release")
                } else {
                    signingConfigs.getByName("debug")
                }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        // BuildConfig.VERSION_NAME is used for the feed poller's User-Agent.
        buildConfig = true
    }

    packaging {
        jniLibs {
            // These ship pre-stripped, so the NDK strip tool emits a noisy
            // "Unable to strip" warning during packaging. Skip them to keep the
            // build log readable.
            keepDebugSymbols += "**/libandroidx.graphics.path.so"
            keepDebugSymbols += "**/libdatastore_shared_counter.so"
        }
    }

    testOptions {
        unitTests {
            // Robolectric needs merged resources to boot an Android runtime for
            // the Room DAO tests.
            isIncludeAndroidResources = true
        }
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)
    implementation(libs.androidx.material.icons.extended)
    implementation(libs.androidx.navigation.compose)

    implementation(libs.androidx.room.runtime)
    implementation(libs.androidx.room.ktx)
    ksp(libs.androidx.room.compiler)

    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.okhttp)
    implementation(libs.jsoup)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.okhttp.mockwebserver)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.core)

    debugImplementation(libs.androidx.ui.tooling)
}

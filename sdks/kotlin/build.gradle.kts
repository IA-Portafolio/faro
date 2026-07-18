import java.time.Duration

plugins {
    // Dependabot subió kotlinx-serialization-json a 1.11.0 compilado con
    // Kotlin 2.3.x → necesitamos compilador 2.3.x o más reciente. Si quedamos
    // en 2.0 / 2.1, falla con "metadata 2.3.0, expected version is 2.0.0".
    kotlin("jvm") version "2.4.10"
    kotlin("plugin.serialization") version "2.4.0"
    `maven-publish`
    signing
    // 1.x adopta la API actual de central.sonatype.com (User Token Bearer
    // auth). 0.0.9 enviaba el Basic Auth en un formato que el portal nuevo
    // ya no acepta — daba 401 "Invalid token" aunque las credentials fueran
    // válidas (curl directo con esas mismas confirma 200/500, no 401).
    id("com.gradleup.nmcp") version "1.6.1"
}

group = "com.iaportafolio"
version = "0.1.0"

repositories { mavenCentral() }

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    // El test usa una mezcla: `org.junit.jupiter.api.{Test, AfterEach}`
    // para anotaciones (necesita junit-jupiter) + `assertEquals` /
    // `assertTrue` / `assertFailsWith` de `kotlin.test` (necesita
    // kotlin-test). El bridge `kotlin-test-junit5` mapea las asserts
    // de kotlin.test al runtime de JUnit 5. `useJUnitPlatform()` abajo
    // asume JUnit 5 en el classpath.
    testImplementation("org.junit.jupiter:junit-jupiter:6.1.1")
    testImplementation(kotlin("test"))
    testImplementation(kotlin("test-junit5"))
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

kotlin { jvmToolchain(17) }

tasks.test {
    useJUnitPlatform()
    // Limita a 4 min — los tests usan sockets y reintentos cortos.
    timeout.set(Duration.ofMinutes(4))
}

java {
    withSourcesJar()
    withJavadocJar()
}

publishing {
    publications {
        create<MavenPublication>("maven") {
            from(components["java"])
            artifactId = "faro"
            pom {
                name.set("faro")
                description.set("Faro SDK for Kotlin (Android + JVM)")
                url.set("https://github.com/IA-Portafolio/faro")
                licenses {
                    license {
                        name.set("MIT License")
                        url.set("https://opensource.org/licenses/MIT")
                    }
                }
                developers {
                    developer {
                        id.set("iaportafolio")
                        name.set("IA Portafolio")
                        email.set("alejo@iaportafolio.com")
                    }
                }
                scm {
                    connection.set("scm:git:git://github.com/IA-Portafolio/faro.git")
                    developerConnection.set("scm:git:ssh://github.com/IA-Portafolio/faro.git")
                    url.set("https://github.com/IA-Portafolio/faro")
                }
            }
        }
    }
}

signing {
    val signingKey: String? = project.findProperty("signingKey") as String?
    val signingPassword: String? = project.findProperty("signingPassword") as String?
    if (signingKey != null && signingPassword != null) {
        useInMemoryPgpKeys(signingKey, signingPassword)
        sign(publishing.publications["maven"])
    } else {
        logger.warn("signingKey/signingPassword no configurados — publish fallará sin firma")
    }
}

// Central Portal nuevo (central.sonatype.com) — el OSSRH legacy
// (s01.oss.sonatype.org) ya no acepta uploads para namespaces nuevos.
// Las credenciales son el "User Token" generado en central.sonatype.com/account.
nmcp {
    // nmcp 1.x cambió la API: ahora es `publishAllPublicationsToCentralPortal`
    // recibe un Action<CentralPortalOptions> con `.set()` sobre Properties.
    // Ojo: la prop se llama `publishingType` (no `publicationType` como en 0.0.x).
    publishAllPublicationsToCentralPortal {
        username.set(providers.gradleProperty("ossrhUsername").orElse(""))
        password.set(providers.gradleProperty("ossrhPassword").orElse(""))
        // AUTOMATIC: tras validación, se publica solo. USER_MANAGED queda
        // en staging para aprobación manual desde el portal.
        publishingType.set("AUTOMATIC")
    }
}

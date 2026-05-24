plugins {
    kotlin("jvm") version "1.9.23"
    kotlin("plugin.serialization") version "1.9.23"
    `maven-publish`
    signing
    id("com.gradleup.nmcp") version "0.0.9"
}

group = "com.iaportafolio"
version = "0.1.0"

repositories { mavenCentral() }

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")
}

kotlin { jvmToolchain(17) }

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
    publishAllPublications {
        username = providers.gradleProperty("ossrhUsername").orElse("")
        password = providers.gradleProperty("ossrhPassword").orElse("")
        // AUTOMATIC: tras validación, se publica solo. USER_MANAGED queda
        // en staging para aprobación manual desde el portal.
        publicationType = "AUTOMATIC"
    }
}

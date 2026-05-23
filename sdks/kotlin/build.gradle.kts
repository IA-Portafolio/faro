plugins {
    kotlin("jvm") version "1.9.23"
    kotlin("plugin.serialization") version "1.9.23"
    `maven-publish`
    signing
}

group = "com.iaportafolio"
version = "0.1.0"

repositories { mavenCentral() }

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
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
    repositories {
        maven {
            name = "ossrh"
            val releasesUrl = "https://s01.oss.sonatype.org/service/local/staging/deploy/maven2/"
            val snapshotsUrl = "https://s01.oss.sonatype.org/content/repositories/snapshots/"
            url = uri(if (version.toString().endsWith("SNAPSHOT")) snapshotsUrl else releasesUrl)
            credentials {
                username = (project.findProperty("ossrhUsername") as String?) ?: System.getenv("OSSRH_USERNAME")
                password = (project.findProperty("ossrhPassword") as String?) ?: System.getenv("OSSRH_PASSWORD")
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
        logger.warn("signingKey/signingPassword no configurados — la publicación a Maven Central fallará sin firma")
    }
}

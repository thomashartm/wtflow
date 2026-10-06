# Build: docker build -f docker/indexers.Dockerfile -t ghcr.io/aderiserp/wtflow-indexers .
FROM rust:1.80-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY schema ./schema
RUN cargo build --release --locked -p wtflow-cli

FROM eclipse-temurin:21-jdk-jammy AS java
FROM node:22-bookworm-slim
COPY --from=java /opt/java/openjdk /opt/java/openjdk
ENV JAVA_HOME=/opt/java/openjdk
ENV PATH="/opt/java/openjdk/bin:${PATH}"
ARG SCIP_JAVA_VERSION=0.13.1
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git unzip maven \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g @sourcegraph/scip-typescript @sourcegraph/scip-python \
    && curl -fsSL https://git.io/coursier-cli -o /tmp/coursier \
    && chmod +x /tmp/coursier \
    && /tmp/coursier bootstrap --standalone -o /usr/local/bin/scip-java \
       "org.scip-code:scip-java:${SCIP_JAVA_VERSION}" --main org.scip_code.scip_java.ScipJava \
    && rm /tmp/coursier
COPY --from=build /build/target/release/wtflow /usr/local/bin/wtflow
WORKDIR /src
CMD ["wtflow", "index"]
# Gradle projects may also use their committed wrapper.
ARG GRADLE_VERSION=8.14.3
RUN curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" -o /tmp/gradle.zip \
    && unzip -q /tmp/gradle.zip -d /opt \
    && ln -s "/opt/gradle-${GRADLE_VERSION}/bin/gradle" /usr/local/bin/gradle \
    && rm /tmp/gradle.zip

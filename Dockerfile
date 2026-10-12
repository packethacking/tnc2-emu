# Build stage: runs on the build machine and cross compiles a static musl
# binary for the target platform, so multi-arch builds need no emulation.
FROM --platform=$BUILDPLATFORM rust:1-slim-bookworm AS build
ARG TARGETPLATFORM
RUN apt-get update \
 && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf libc6-dev \
 && rm -rf /var/lib/apt/lists/*
RUN case "$TARGETPLATFORM" in \
      linux/arm64) echo aarch64-unknown-linux-musl ;; \
      linux/arm/v7) echo armv7-unknown-linux-musleabihf ;; \
      *) echo x86_64-unknown-linux-musl ;; \
    esac > /rust-target \
 && rustup target add "$(cat /rust-target)"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER=arm-linux-gnueabihf-gcc
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY crates ./crates
RUN cargo build --release --locked --target "$(cat /rust-target)" -p tnc2-emu \
 && cp "target/$(cat /rust-target)/release/tnc2-emu" /tnc2-emu

# Runtime stage: just the binary and the EPROM images
FROM scratch
COPY --from=build /tnc2-emu /tnc2-emu
COPY rom/THENET1.X1J rom/THENET2.X1J rom/COPYRITE.TXT /rom/
ENV TNC2_ROM1=/rom/THENET1.X1J \
    TNC2_ROM2=/rom/THENET2.X1J \
    TNC2_RAM_FILE=/data/tnc2.ram \
    TNC2_KISS_PORT=8001 \
    TNC2_CONSOLE_PORT=8002 \
    TNC2_MONITOR=1
VOLUME /data
EXPOSE 8001 8002
ENTRYPOINT ["/tnc2-emu"]

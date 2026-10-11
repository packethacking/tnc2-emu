# Build stage: static musl binary
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# Runtime stage
FROM alpine:3.20
RUN apk add --no-cache tini
COPY --from=build /src/target/release/tnc2-emu /usr/local/bin/tnc2-emu
COPY rom/THENET1.X1J rom/THENET2.X1J rom/COPYRITE.TXT /opt/tnc2/rom/
ENV TNC2_ROM1=/opt/tnc2/rom/THENET1.X1J \
    TNC2_ROM2=/opt/tnc2/rom/THENET2.X1J \
    TNC2_RAM_FILE=/data/tnc2.ram \
    TNC2_KISS_PORT=8001 \
    TNC2_CONSOLE_PORT=8002 \
    TNC2_CALL= \
    TNC2_ALIAS= \
    TNC2_MONITOR=1
VOLUME /data
EXPOSE 8001 8002
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/tnc2-emu"]

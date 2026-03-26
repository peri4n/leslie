FROM rust:1.94-bookworm AS builder

# Build dependencies for tonic/prost codegen
RUN apt-get update \
 && apt-get install -y --no-install-recommends protobuf-compiler \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache deps first
COPY Cargo.toml Cargo.lock ./
COPY proto ./proto
RUN mkdir -p src && echo "fn main(){}" > src/main.rs \
 && cargo build --release || true

# Real sources
COPY src ./src
COPY build.rs ./

RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates netcat-openbsd \
 && rm -rf /var/lib/apt/lists/* \
 && update-ca-certificates

COPY --from=builder /app/target/release/leslie /usr/local/bin/leslie

# gRPC + Prometheus metrics
EXPOSE 50051 9464

ENV RUST_LOG=info
ENV PROMETHEUS_BIND=0.0.0.0:9464

ENTRYPOINT ["/usr/local/bin/leslie"]

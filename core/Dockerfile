FROM rust:slim AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
# Create dummy main to cache deps
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true

# Remove dummy artifacts so real build triggers
RUN rm -rf src target/release/sensusai-core target/release/deps/sensusai_core*

COPY src ./src
# Touch to ensure cargo sees new source
RUN touch src/main.rs
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/sensusai-core /usr/local/bin/sensusai-core

EXPOSE 4000
CMD ["sensusai-core"]

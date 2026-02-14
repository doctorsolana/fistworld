# Build stage - compile the Rust server
FROM rust:1.92-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies (including Wayland, X11, audio libs for Bevy)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libwayland-dev \
    libxkbcommon-dev \
    libasound2-dev \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY server ./server

# Create dummy workspace members to satisfy Cargo
# Client
RUN mkdir -p client/src && \
    echo '[package]' > client/Cargo.toml && \
    echo 'name = "client"' >> client/Cargo.toml && \
    echo 'version = "0.1.0"' >> client/Cargo.toml && \
    echo 'edition = "2021"' >> client/Cargo.toml && \
    echo 'fn main() {}' > client/src/main.rs

# Tools/collider_baker
RUN mkdir -p tools/collider_baker/src && \
    echo '[package]' > tools/collider_baker/Cargo.toml && \
    echo 'name = "collider_baker"' >> tools/collider_baker/Cargo.toml && \
    echo 'version = "0.1.0"' >> tools/collider_baker/Cargo.toml && \
    echo 'edition = "2021"' >> tools/collider_baker/Cargo.toml && \
    echo 'fn main() {}' > tools/collider_baker/src/main.rs

# Build release binary (server only)
RUN cargo build --release --package server

# Runtime stage - minimal image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary
COPY --from=builder /app/target/release/server /usr/local/bin/server

# Copy colliders data (server needs this at runtime)
RUN mkdir -p /usr/local/bin/client/assets
COPY client/assets/colliders.bin /usr/local/bin/client/assets/colliders.bin

# Set working directory so relative paths work
WORKDIR /usr/local/bin

# Expose UDP port for game traffic
EXPOSE 5000/udp

# Run the server
CMD ["server"]

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

# Editor and tools/terrain_ktx_builder. Both are workspace members in Cargo.toml
# but were added AFTER this file last stubbed its members, so cargo failed to
# load the workspace ("failed to load manifest for workspace member") and the
# image could not build at all. Every member needs a stub here or an exclude
# there; adding a member without one breaks the deploy silently.
RUN mkdir -p editor/src && \
    echo '[package]' > editor/Cargo.toml && \
    echo 'name = "editor"' >> editor/Cargo.toml && \
    echo 'version = "0.1.0"' >> editor/Cargo.toml && \
    echo 'edition = "2021"' >> editor/Cargo.toml && \
    echo 'fn main() {}' > editor/src/main.rs

RUN mkdir -p tools/terrain_ktx_builder/src && \
    echo '[package]' > tools/terrain_ktx_builder/Cargo.toml && \
    echo 'name = "terrain_ktx_builder"' >> tools/terrain_ktx_builder/Cargo.toml && \
    echo 'version = "0.1.0"' >> tools/terrain_ktx_builder/Cargo.toml && \
    echo 'edition = "2021"' >> tools/terrain_ktx_builder/Cargo.toml && \
    echo 'fn main() {}' > tools/terrain_ktx_builder/src/main.rs

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

# The MAP is not optional. The server does `init_resource::<WorldTerrain>()` at
# boot, whose Default runs the map loader, which panics outright if it cannot
# find the authored map ("Failed to load authored map"). Without this the
# container built and then died on its first tick.
#
# map.ron is ~13MB, most of it baked prop spawns that are already derivable from
# the seed recipe — worth trimming later, but the server needs the file as-is
# today because the loader reads spawns from it rather than regenerating them.
COPY client/assets/maps /usr/local/bin/client/assets/maps

# Set working directory so relative paths work
WORKDIR /usr/local/bin

# Expose UDP port for game traffic
EXPOSE 5000/udp

# Run the server
CMD ["server"]

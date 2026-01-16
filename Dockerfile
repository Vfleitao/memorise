# ============================================
# Stage 1: Build the Rust server
# ============================================
FROM rust:1.85-slim AS builder

# Install protobuf compiler (required for prost-build)
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy only the files needed for dependency resolution first (better caching)
COPY memorize-core/Cargo.toml memorize-core/Cargo.toml
COPY memorize-proto/Cargo.toml memorize-proto/Cargo.toml
COPY memorize-proto/build.rs memorize-proto/build.rs
COPY memorize-server/Cargo.toml memorize-server/Cargo.toml

# Create dummy source files to build dependencies
RUN mkdir -p memorize-core/src memorize-proto/src memorize-server/src && \
    echo "pub fn dummy() {}" > memorize-core/src/lib.rs && \
    echo "pub fn dummy() {}" > memorize-proto/src/lib.rs && \
    echo "fn main() {}" > memorize-server/src/main.rs

# Copy proto file (needed for memorize-proto build)
COPY memorize-proto/proto memorize-proto/proto

# Build dependencies (this layer is cached if Cargo.toml files don't change)
RUN cd memorize-proto && cargo build --release 2>/dev/null || true
RUN cd memorize-server && cargo build --release 2>/dev/null || true

# Now copy the actual source code
COPY memorize-core/src memorize-core/src
COPY memorize-proto/src memorize-proto/src
COPY memorize-server/src memorize-server/src

# Touch source files to ensure they're rebuilt
RUN touch memorize-core/src/lib.rs memorize-proto/src/lib.rs memorize-server/src/main.rs

# Build the release binary
RUN cd memorize-server && cargo build --release

# ============================================
# Stage 2: Create minimal runtime image
# ============================================
FROM debian:bookworm-slim AS runtime

# Install ca-certificates for potential HTTPS connections
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd --create-home --shell /bin/bash memorize

WORKDIR /app

# Copy the built binary from builder stage
COPY --from=builder /build/memorize-server/target/release/memorize-server /app/memorize-server

# Change ownership to non-root user
RUN chown -R memorize:memorize /app

USER memorize

# Environment variables with defaults
ENV MEMORIZE_HOST=0.0.0.0
ENV MEMORIZE_PORT=50051
ENV MEMORIZE_CLEANUP_INTERVAL=60
# Max storage in MB (default 100MB, set to 0 for unlimited - use with caution!)
ENV MEMORIZE_MAX_STORAGE_MB=100
# MEMORIZE_API_KEY is intentionally not set by default (auth disabled)

# Expose the gRPC port
EXPOSE 50051

# Health check (gRPC health check would be better, but this ensures the process is running)
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD pgrep memorize-server || exit 1

# Run the server
ENTRYPOINT ["/app/memorize-server"]

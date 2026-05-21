# ==========================================
# STAGE 1: Builder
# ==========================================
FROM rust:1.75-slim AS builder

WORKDIR /usr/src/korg

# Install build dependencies (openssl, pkg-config, etc. if needed in the future)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests and source code
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build in release mode
RUN cargo build --release

# ==========================================
# STAGE 2: Minimal Runner
# ==========================================
FROM debian:bookworm-slim AS runner

WORKDIR /app

# Install runtime dependencies and basic tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/korg/target/release/korg /usr/local/bin/korg

# Create necessary directories for state persistence (blackboard, ktrans journals, contracts)
RUN mkdir -p /tmp/korg/blackboard /tmp/korg/ktrans /tmp/korg/contracts /tmp/korg/worktrees \
    && chmod -R 777 /tmp/korg

# Set environment variables for high performance
ENV PAGER=cat
ENV RUST_LOG=info

# Default command starts the cyberpunk welcome banner
ENTRYPOINT ["/usr/local/bin/korg"]
CMD []

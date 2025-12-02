FROM rust:1.83

# Install extra tooling you might want inside the devcontainer.
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        git \
        curl \
        pkg-config \
        libssl-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install helpful Rust tools.
RUN rustup component add clippy rustfmt

# Default command is controlled by docker-compose (sleep infinity).

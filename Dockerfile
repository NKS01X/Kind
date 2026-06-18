# Build stage
FROM rust:slim-bullseye AS builder

# Install a modern protoc directly from GitHub (the Debian APT version is too old)
RUN apt-get update && apt-get install -y curl unzip && rm -rf /var/lib/apt/lists/*
RUN curl -Lo /tmp/protoc.zip https://github.com/protocolbuffers/protobuf/releases/download/v25.3/protoc-25.3-linux-x86_64.zip \
    && unzip /tmp/protoc.zip -d /usr/local \
    && rm /tmp/protoc.zip

WORKDIR /usr/src/kind
COPY . .

# Build the release binary
RUN cargo build --release

# Runtime stage
FROM debian:bullseye-slim

WORKDIR /app

# Create a data directory for persistent volumes
RUN mkdir -p /app/data

# Copy the compiled binaries and default schema
COPY --from=builder /usr/src/kind/target/release/kind /usr/local/bin/kind
COPY --from=builder /usr/src/kind/target/release/kindctl /usr/local/bin/kindctl
COPY --from=builder /usr/src/kind/schema.ksl /app/data/schema.ksl

# Set environment variables to point to the data directory
ENV KIND_SNAPSHOT_PATH="/app/data/snapshot.json"
ENV KIND_SCHEMA_PATH="/app/data/schema.ksl"
ENV KIND_WAL_PATH="/app/data/kind.wal"

# Expose the gRPC port
EXPOSE 50051

# Run the database
CMD ["kind"]

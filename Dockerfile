# Multi-stage Dockerfile for parlor-room matchmaking microservice
# Stage 1: Build environment
FROM rust:1.75 as builder

# Set the working directory
WORKDIR /app

# Create app user for security
RUN groupadd -r app && useradd -r -g app app

# Install required system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the entire project
COPY . .

# Build the actual application
RUN cargo build --release --bin parlor-room

# Strip the binary to reduce size
RUN strip target/release/parlor-room

# Stage 2: Runtime environment using distroless
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy the CA certificates for HTTPS/TLS connections
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy the compiled binary from builder stage
COPY --from=builder /app/target/release/parlor-room /usr/local/bin/parlor-room

# Create necessary directories with proper permissions
USER nonroot:nonroot

# Expose default ports (can be overridden)
EXPOSE 8080
EXPOSE 9090

# Set environment variables for production
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

# Default command
ENTRYPOINT ["/usr/local/bin/parlor-room"]
CMD ["--config", "/etc/parlor-room/config.toml"] 
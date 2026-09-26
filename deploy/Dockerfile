FROM rust:1.98.1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release --locked -p better-commerce-core --bin better-commerce-core --bin migrate --bin bc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/target/release/better-commerce-core /usr/local/bin/better-commerce-core
COPY --from=builder /workspace/target/release/migrate /usr/local/bin/migrate
COPY --from=builder /workspace/target/release/bc /usr/local/bin/bc
EXPOSE 3000
CMD ["/usr/local/bin/better-commerce-core"]

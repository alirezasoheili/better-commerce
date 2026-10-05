FROM node:24.19.0-bookworm-slim AS frontend
WORKDIR /workspace
COPY frontend frontend
RUN npm ci --prefix frontend/storefront && npm run typecheck --prefix frontend/storefront && npm run build --prefix frontend/storefront
RUN npm ci --prefix frontend/admin && npm run typecheck --prefix frontend/admin && npm run build --prefix frontend/admin

FROM rust:1.98.1-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY core/Cargo.toml core/Cargo.toml
COPY core/src core/src
COPY core/migrations core/migrations
COPY modules/example/Cargo.toml modules/example/Cargo.toml
COPY modules/example/src modules/example/src
COPY modules/example/migrations modules/example/migrations
COPY modules/example-consumer/Cargo.toml modules/example-consumer/Cargo.toml
COPY modules/example-consumer/src modules/example-consumer/src
COPY modules/example-transport/Cargo.toml modules/example-transport/Cargo.toml
COPY modules/example-transport/src modules/example-transport/src
RUN cargo build --release --locked -p better-commerce-core --bin better-commerce-core --bin migrate --bin bc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/target/release/better-commerce-core /usr/local/bin/better-commerce-core
COPY --from=builder /workspace/target/release/migrate /usr/local/bin/migrate
COPY --from=builder /workspace/target/release/bc /usr/local/bin/bc
COPY --from=frontend /workspace/frontend/storefront/dist /app/storefront
COPY --from=frontend /workspace/frontend/admin/dist /app/admin
EXPOSE 3000
CMD ["/usr/local/bin/better-commerce-core"]

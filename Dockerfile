# Build stage
FROM rust:1.83-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY oxeye-backend/Cargo.toml oxeye-backend/
COPY oxeye-db/Cargo.toml oxeye-db/

RUN mkdir -p oxeye-backend/src oxeye-db/src && \
    echo "fn main() {}" > oxeye-backend/src/main.rs && \
    echo "pub fn dummy() {}" > oxeye-backend/src/lib.rs && \
    echo "pub fn dummy() {}" > oxeye-db/src/lib.rs

RUN cargo build --release 2>/dev/null || true

COPY oxeye-backend/src oxeye-backend/src
COPY oxeye-db/src oxeye-db/src

RUN touch oxeye-backend/src/main.rs && cargo build --release

# Runtime stage - scratch with just the binary and CA certs
FROM scratch

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/oxeye-backend /oxeye-backend

ENV DATABASE_PATH=/data/oxeye.db
ENV PORT=3000

EXPOSE 3000

ENTRYPOINT ["/oxeye-backend"]
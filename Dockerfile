# build stage
FROM docker.io/rust:1-bookworm AS build
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake pkg-config libssl-dev libclang-dev \
    ffmpeg ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY backend ./backend
WORKDIR /app/backend
RUN cargo build --release

# runtime stage
FROM docker.io/debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg libssl3 ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/backend/target/release/persona-ai /usr/local/bin/persona-ai
COPY --from=build /app/backend/migrations /app/migrations
ENV MIGRATIONS_DIR=/app/migrations
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/persona-ai"]

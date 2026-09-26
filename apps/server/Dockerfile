# Local-dev image for fraglink-server (used by `docker compose --profile server up`).
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fraglink-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/fraglink-server /usr/local/bin/fraglink-server
EXPOSE 8787
CMD ["fraglink-server"]

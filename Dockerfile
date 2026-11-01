# infodesk CLI for the home k3s cluster (amd64). Build on the WSL node; see docs/15-home-server.md.
FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p infodesk --locked

FROM build AS test
RUN cargo test -p infodesk-core -p infodesk --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --no-create-home infodesk
COPY --from=build /src/target/release/infodesk /usr/local/bin/infodesk
USER 10001
ENTRYPOINT ["infodesk"]

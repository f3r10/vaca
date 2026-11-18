# API image for Fly. The build context is the repo root.
# The server key is not copied. The entrypoint writes it from a Fly secret at start.
FROM rust:1.99-bookworm AS build

WORKDIR /src/server
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin vaca

COPY --from=build /src/server/target/release/vaca-server /usr/local/bin/vaca-server
COPY deploy/api-entrypoint.sh /usr/local/bin/api-entrypoint.sh
RUN chmod 755 /usr/local/bin/vaca-server /usr/local/bin/api-entrypoint.sh

EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/api-entrypoint.sh"]

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends masscan sudo ca-certificates libpcap0.8 \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Railway containers have no raw socket access, so masscan cannot run there
ENV SCAN_METHOD=tcp
ENV RUST_LOG=info
COPY --from=build /src/target/release/ServerSeekerV2 /usr/local/bin/ServerSeekerV2
COPY --from=build /src/migrations /app/migrations
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ENTRYPOINT ["docker-entrypoint.sh"]

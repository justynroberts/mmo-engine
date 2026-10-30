# Build the UI, build the server and bots, ship a small runtime image.
FROM node:22-slim AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

FROM rust:1-slim AS build
# Fat LTO links in one big step; two jobs keeps a small build host from running out of memory.
ENV CARGO_BUILD_JOBS=2
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p server -p bots -p worldgen

FROM debian:stable-slim
# curl for the platform's health check (it runs inside the container).
RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app && mkdir /data && chown app /data
COPY --from=build /src/target/release/server /src/target/release/bots /src/target/release/worldgen /usr/local/bin/
COPY --from=ui /ui/dist /opt/ui
# What a fresh /data volume starts with: the game file, the world and the blocks built so far.
COPY deploy/seed /opt/seed
# Other worlds to start from, picked by MMO_SEED (entrypoint.sh); a big one ships its recipe and is made on first start.
COPY deploy/hibernia-65536 /opt/seeds/hibernia-65536
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
USER app
WORKDIR /data
# game.json and server.key live in /data: mount a volume there to keep them.
# The admin interface only binds to loopback (it has no login), so run with host
# networking, e.g.  docker run --network host -v mmo-data:/data mmo-engine
EXPOSE 7777/udp
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# Native client only by default: the web port serves the data it fetches, not the browser play page (a run that
# passes its own arguments, like the local docker-compose.yml, decides for itself).
CMD ["server", "--ui-dir", "/opt/ui", "--no-browser"]

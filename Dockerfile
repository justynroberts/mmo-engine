# Build the UI, build the server and bots, ship a small runtime image.
FROM node:22-slim AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p server -p bots

FROM debian:stable-slim
RUN useradd --create-home --uid 10001 app && mkdir /data && chown app /data
COPY --from=build /src/target/release/server /src/target/release/bots /usr/local/bin/
COPY --from=ui /ui/dist /opt/ui
USER app
WORKDIR /data
# game.json and server.key live in /data: mount a volume there to keep them.
# The admin interface only binds to loopback (it has no login), so run with host
# networking, e.g.  docker run --network host -v mmo-data:/data mmo-engine
EXPOSE 7777/udp
CMD ["server", "--ui-dir", "/opt/ui"]

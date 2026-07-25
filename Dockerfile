# syntax=docker/dockerfile:1
#
# Immagine finale di av1webconverter: un solo container con il binario
# self-contained (frontend embedded) e la build custom di ffmpeg con SVT-AV1.
#
# I binari ffmpeg/ffprobe vengono presi dall'immagine costruita con
# `Dockerfile.ffmpeg-svtav1`, che resta l'unica fonte di verità per versioni e
# checksum. Va costruita (o scaricata) prima:
#
#   docker build -f Dockerfile.ffmpeg-svtav1 \
#     -t local/ffmpeg-svtav1:8.1.2-svt4.2.0-vmaf3.1.0 .
#   docker build -t av1webconverter:latest .
#
# Per usare un tag pubblicato su Docker Hub:
#   docker build --build-arg FFMPEG_IMAGE=utente/ffmpeg-svtav1:tag -t av1webconverter:latest .

ARG FFMPEG_IMAGE=local/ffmpeg-svtav1:8.1.2-svt4.2.0-vmaf3.1.0
ARG RUST_IMAGE=rust:1.96-bookworm

FROM ${FFMPEG_IMAGE} AS ffmpeg

FROM ${RUST_IMAGE} AS builder

WORKDIR /src

# Le dipendenze cambiano molto meno del codice: si compilano prima, da sole,
# così il layer resta in cache fra una build e l'altra.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
# Il touch forza la ricompilazione dei sorgenti veri sopra il layer in cache.
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --locked \
    && strip target/release/av1webconverter

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818

# libopus0 è l'unica libreria dinamica richiesta dal ffmpeg statico,
# ca-certificates serve alle notifiche Slack in HTTPS, curl al healthcheck.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libopus0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 av1

COPY --from=ffmpeg /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg /usr/local/bin/ffprobe /usr/local/bin/ffprobe
COPY --from=builder /src/target/release/av1webconverter /usr/local/bin/av1webconverter
COPY config.example.toml /etc/av1webconverter/config.example.toml

# Cartelle di lavoro: input, output, stato dei job e log.
RUN mkdir -p /data/in /data/out /data/state /data/logs && chown -R av1:av1 /data

ENV AV1WC_INPUT_DIR=/data/in \
    AV1WC_OUTPUT_DIR=/data/out \
    AV1WC_DATA_DIR=/data/state \
    AV1WC_LOG_DIR=/data/logs \
    AV1WC_CONFIG=/etc/av1webconverter/config.toml \
    AV1WC_BIND=0.0.0.0 \
    AV1WC_PORT=8080

VOLUME ["/data"]
EXPOSE 8080
USER av1
WORKDIR /data

# /healthz non richiede credenziali, quindi il probe funziona anche con la
# basic auth abilitata.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${AV1WC_PORT}/healthz" > /dev/null

LABEL org.opencontainers.image.title="av1webconverter"
LABEL org.opencontainers.image.description="Conversione video AV1 (SVT-AV1) con frontend web embedded"

ENTRYPOINT ["/usr/local/bin/av1webconverter"]

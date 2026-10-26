# Speed Demon - Track A (Ingest). Per-track scored image.
#
# Built with the track folder as the build context, exactly as the events
# page shows:
#   docker build --network=none -t speeddemon/<team>-A:<commit> track-A
#
# Both stages are digest-pinned, allowlisted base images (these are the
# digests published on speeddemon.dev). Nothing else enters the image: no
# COPY --from other images, no ADD, no network, no cache mounts. The final
# image contains exactly one file: the built binary.

FROM rust:1-slim-trixie@sha256:1a939748a7cbab172de91a014e3804a97af1394a8c844faa67fce9401c4e40fb AS build
COPY . /src
WORKDIR /src
RUN ./build.sh

FROM debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=build /src/run /app/run
WORKDIR /app
ENTRYPOINT ["/app/run"]
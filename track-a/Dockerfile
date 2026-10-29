# Speed Demon - Track A (Ingest). Per-track scored image.
#
# The track folder is the build context. Both stages use digest-pinned,
# allowlisted base images (the digests published on speeddemon.dev).
# The build is offline and the final image contains one file: the binary.

FROM rust:1-slim-trixie@sha256:1a939748a7cbab172de91a014e3804a97af1394a8c844faa67fce9401c4e40fb AS build
COPY . /src
WORKDIR /src
RUN ./build.sh

FROM debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=build /src/run /app/run
WORKDIR /app
ENTRYPOINT ["/app/run"]
# Speed Demon - Track B (Index). Per-track scored image.
#
# The track folder is the build context. Both stages use digest-pinned,
# allowlisted base images (the digests published on speeddemon.dev).
# The build is offline and the final image contains one file: the binary.

# The llvm-tools variant of the rust image: same toolchain, plus llvm-profdata.
# PGO needs the profiler to match rustc exactly, and this is the allowlisted
# image that carries it (verified by tooling/check_allowlist.py). It is the
# same digest Track A and Track C build against. This track previously used the
# plain rust:1-slim-trixie image, which has NO llvm-profdata, so the PGO path
# its build.sh described and priced at "~5-15% on T" could never run.
FROM halloumee/speed-demon-rust:1.99.1-trixie-llvm-tools@sha256:13e4ee7135d8eac6c56476c7d1aee201f36479f4745fd5d0e6b4529f4e13251a AS build
COPY . /src
WORKDIR /src
RUN ./build.sh

FROM debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=build /src/run /app/run
WORKDIR /app
ENTRYPOINT ["/app/run"]
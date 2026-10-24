# Standalone single-track image (judge-readable).
#
# The SCORED image is built from the repository root with the root
# Dockerfile: it packages all five tracks and dispatches on $TRACK at run
# time. This file exists so a judge can read one track's image end to end
# without wading through five.
#
# BUILD CONTEXT: the REPOSITORY ROOT — not this directory.
#
#   docker build --network=none -f tracks/D-traverse/Dockerfile -t speeddemon/D .
#                                                                          ^
#                                          from the repository root
#
# The context must be the root because the image needs `core/`, `run` and
# `build-one.sh`, which live there. Building from inside the track directory
# cannot work, and the previous version of this file assumed it could.
FROM debian:13-slim

COPY . /app
WORKDIR /app

# Build only THIS track. `build-one.sh` compiles the core once and then the
# single binary, offline, with rustc directly — no cargo, no registry, no
# network.
RUN chmod +x /app/build-one.sh /app/run && TRACK=D /app/build-one.sh

# The dispatcher selects the binary from $TRACK at run time. In this
# single-track image only track D exists, so the default is D.
ENV TRACK=D

ENTRYPOINT ["/app/run"]

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
#
# The base image is DIGEST-PINNED. The rules announced before the Oct 28
# freeze require it: "The final image must build from a published,
# digest-pinned toolchain image, so code can't hide in a custom base image."
# (docs/RULE-CHANGES-oct28.md §3)
#
# The reference comes from tooling/base-image.txt, which is the single source
# of truth — the root Dockerfile and these five must not drift apart, and
# tooling/check_base_pinned.py enforces that they do not.
FROM debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad

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

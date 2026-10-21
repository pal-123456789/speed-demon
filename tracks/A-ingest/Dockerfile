# Standalone single-track image (judge-readable). The SCORED image is built
# from the repo root with the root Dockerfile, which packages all five tracks
# and dispatches on \ at run time.
FROM debian:13-slim
COPY . /app
WORKDIR /app
RUN chmod +x /app/build.sh /app/run && /app/build.sh
ENTRYPOINT ["/app/run"]

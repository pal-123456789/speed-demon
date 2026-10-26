#!/bin/sh
# Offline build for THIS track folder. rustc only, no cargo, no registry,
# no network: the scored build runs with --network=none inside the official
# rust:1-slim-trixie build stage, which already ships rustc. Nothing needs
# downloading, so there is nothing to vendor.
set -eu
cd "$(dirname "$0")"
rustc --edition 2021 \
      -C opt-level=3 -C codegen-units=1 \
      -C panic=abort -C strip=symbols \
      -o run src/main.rs
#!/bin/sh
# Verify every track at the pushed commit against its PUBLISHED full-scale hash.
#
# Not `sd-harness verify`: that reads the committed pin, which is sample-scale by
# design, so a full-scale run would compare against the wrong expectation and
# print FAIL for a correct answer. See verification/EXPECTED-HASHES.md.
set -u

case "${1:-all}" in
    a|all) echo "--- track a ---"; sh tooling/measure-full.sh a /d/sd-data/track-a 495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3 summary.txt ;;
esac
case "${1:-all}" in
    b|all) echo "--- track b ---"; sh tooling/measure-full.sh b /d/sd-data/track-b f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927 results.txt ;;
esac
case "${1:-all}" in
    c|all) echo "--- track c ---"; sh tooling/measure-full.sh c /d/sd-data/track-c d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536 aggregates.txt ;;
esac
case "${1:-all}" in
    d|all) echo "--- track d ---"; sh tooling/measure-full.sh d /d/sd-data/track-d a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e distances.txt ;;
esac

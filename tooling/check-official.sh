#!/bin/sh
# Run the organizers' own check.py against a track: real Docker build, pinned base,
# sandbox run, hash comparison. This is the only test that exercises the assembly
# rather than the bare binary.
#
#   usage: sh tooling/check-official.sh <track>
set -u
track="$1"
case "$track" in
  a) H=495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3 ;;
  b) H=f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927 ;;
  c) H=d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536 ;;
  d) H=a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e ;;
  *) echo "usage: sh tooling/check-official.sh a|b|c|d"; exit 2 ;;
esac

cd /mnt/d/SpeedDemon-Campaign || exit 2
echo "=== check.py track $track ==="
python3 check.py "$track" --data "/mnt/d/sd-data/track-$track" --expected "$H" 2>&1 | tail -7

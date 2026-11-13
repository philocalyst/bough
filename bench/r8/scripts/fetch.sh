#!/usr/bin/env bash
# fetch.sh REMOTE_FILE LOCAL_FILE : copy a file from the remote host through rex (base64 lines captured from an
# rx.sh block), verified by sha256 against the remote's. Prints both hashes; exit 1 on mismatch.
H=$(cd "$(dirname "$0")" && pwd)
R=$1; L=$2
T=$(mktemp -d)
cat > $T/f.sh <<EOS
sha256sum $R | sed 's/^/HASH /'
base64 -w 0 $R | fold -w 120000 | sed 's/^/DATA /'
EOS
"$H/rx.sh" $T/f.sh ${3:-120} > $T/out.txt
rh=$(grep '^HASH ' $T/out.txt | head -1 | awk '{print $2}')
grep '^DATA ' $T/out.txt | sed 's/^DATA //' | tr -d '\r\n ' | base64 -d > "$L"
lh=$(shasum -a 256 "$L" | cut -d' ' -f1)
echo "remote $rh"; echo "local  $lh"
rm -rf $T
[ "$rh" = "$lh" ] || { echo "HASH MISMATCH"; exit 1; }

#!/usr/bin/env sh
set -eu

# sample.dmdata.jp exposes an S3-compatible public listing. Direct directory URLs
# currently return 404, so enumerate every VXSE45 object through the prefix API.
events='20230505215718 20230513161031 20230906063640 20240101161010 20240101161133'
destination=${1:-tests/fixtures/downloaded}
mkdir -p "$destination"

for event in $events; do
  event_dir="$destination/$event"
  mkdir -p "$event_dir"
  listing=$(mktemp)
  curl -L --fail --silent --show-error \
    "https://sample.dmdata.jp/?prefix=eew/$event/vxse45/xml/" \
    -o "$listing"
  # The S3 listing is normally one long XML line containing many <Key> nodes.
  sed 's#</Key>#</Key>\
#g' "$listing" | sed -n 's#.*<Key>\([^<]*\)</Key>.*#\1#p' | while IFS= read -r key; do
    curl -L --fail --silent --show-error \
      "https://sample.dmdata.jp/$key" \
      -o "$event_dir/$(basename "$key")"
  done
  rm -f "$listing"
done

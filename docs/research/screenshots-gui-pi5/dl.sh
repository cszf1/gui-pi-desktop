set -e
mkdir -p /data/workspace/rel && cd /data/workspace/rel
for tag in build-f2b6d6b build-f2b6d6b-testhooks; do
  mkdir -p $tag; cd $tag
  curl -s -m 30 https://api.github.com/repos/cszf1/gui-pi-desktop/releases/tags/$tag | python3 -c 'import sys,json; d=json.load(sys.stdin); print("\n".join(a["name"]+" "+a["browser_download_url"]+" "+str(a["size"]) for a in d["assets"]))' > assets.txt
  cat assets.txt | awk '{print $1, $3}'
  while read n u s; do [ -f "$n" ] && [ $(stat -c%s "$n") = "$s" ] || curl -sL -m 300 -o "$n" "$u"; done < assets.txt
  echo "-- sha256sum -c ($tag)"; sha256sum -c SHA256SUMS 2>&1 | tail -5
  cd ..
done

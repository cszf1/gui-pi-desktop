# dl2.sh TAG... : download prerelease assets and verify SHA256SUMS
set -e
for tag in "$@"; do
  mkdir -p /data/workspace/rel/$tag; cd /data/workspace/rel/$tag
  curl -s -m 30 https://api.github.com/repos/cszf1/gui-pi-desktop/releases/tags/$tag | python3 -c 'import sys,json; d=json.load(sys.stdin); print("\n".join(a["name"]+" "+a["browser_download_url"]+" "+str(a["size"]) for a in d["assets"]))' > assets.txt
  while read n u s; do [ -f "$n" ] && [ $(stat -c%s "$n") = "$s" ] || curl -sL -m 300 -o "$n" "$u"; done < assets.txt
  echo "-- $tag: $(awk '{print $1}' assets.txt | tr '\n' ' ')"; sha256sum -c SHA256SUMS 2>&1
  chmod +x *.AppImage
done

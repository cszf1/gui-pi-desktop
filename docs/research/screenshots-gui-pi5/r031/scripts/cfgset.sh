# cfgset.sh KEY.PATH VALUE | restore : edit the app's config.json (backup kept); prints only the changed field
C=$(ls -d ~/.config/io.github.cszf1.gui-pi-desktop)/config.json
if [ "$1" = restore ]; then cp $C.bak $C; python3 -c "import json;d=json.load(open('$C'));print('main.model =',d['main']['model'])"; exit; fi
[ -f $C.bak ] || cp $C $C.bak
python3 - "$C" "$1" "$2" <<'PY'
import json, sys
c, path, val = sys.argv[1:]
d = json.load(open(c)); o = d; ks = path.split(".")
for k in ks[:-1]: o = o[k]
o[ks[-1]] = val; json.dump(d, open(c, "w"), indent=1); print(path, "=", val)
PY

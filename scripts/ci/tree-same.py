#!/usr/bin/env python3
"""Two folder trees are the same byte for byte: same entries, same kinds (a symlink stays a symlink with the same
target, never a copied file), same file bytes, same executable bits. Used on macOS for the nested CuaDriver.app
(Cua's signature covers its bundle layout, symlinks included).
usage: tree-same.py A B"""
import hashlib
import os
import stat
import sys


def scan(root):
    out = {}
    for d, dirs, files in os.walk(root, followlinks=False):
        for n in dirs + files:
            p = os.path.join(d, n)
            rel = os.path.relpath(p, root)
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                out[rel] = ("link", os.readlink(p))
            elif stat.S_ISDIR(st.st_mode):
                out[rel] = ("dir",)
            else:
                with open(p, "rb") as f:
                    out[rel] = ("file", hashlib.sha256(f.read()).hexdigest(), bool(st.st_mode & 0o111))
    return out


a, b = scan(sys.argv[1]), scan(sys.argv[2])
bad = [f"only in {sys.argv[1]}: {k}" for k in sorted(set(a) - set(b))]
bad += [f"only in {sys.argv[2]}: {k}" for k in sorted(set(b) - set(a))]
bad += [f"differs: {k}: {a[k]} vs {b[k]}" for k in sorted(set(a) & set(b)) if a[k] != b[k]]
links = sum(1 for v in a.values() if v[0] == "link")
print(f"{len(a)} entries, {links} symlinks in {sys.argv[1]}")
for line in bad[:40]:
    print(line)
if bad:
    print(f"{len(bad)} differences")
    sys.exit(1)
print("same tree (symlinks kept)")

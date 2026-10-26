# scripted person request to the shell's IPC (test only): ipc.py KIND N_REQUESTS TIMEOUT_S
import os, sys, json, socket, time, glob
kind, n, tmo = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
pid = None
for p in glob.glob("/proc/[0-9]*/cmdline"):
    try:
        q = p.split("/")[2]
        if b"GUI_PI_HUMAN_TOKEN=" in open(f"/proc/{q}/environ", "rb").read(): pid = q
    except Exception: pass
env = dict(x.split("=", 1) for x in open(f"/proc/{pid}/environ", "rb").read().decode().split("\0") if "=" in x)
path, tok = env["GUI_PI_HUMAN_IPC"], env["GUI_PI_HUMAN_TOKEN"]
print("pi pid", pid, "socket", path, "token len", len(tok))
schemas = {"ask_help": {"type": "object", "properties": {"choice": {"type": "string", "enum": ["did_it", "hint", "try_other"]}, "note": {"type": "string"}}, "required": ["choice"]},
           "takeover": {"type": "object", "properties": {"done": {"type": "boolean"}}, "required": ["done"]},
           "answer": {"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"]}}
meta = {"ask_help": {"tried": ["clicked Save report twice", "pixel click on Save report"], "reason": "two failed steps in a row"},
        "takeover": {"steps": ["type the password", "press Login"]}, "answer": {"options": [{"label": "A", "value": "a"}]}}
for i in range(n):
    s = socket.socket(socket.AF_UNIX); s.connect(path)
    rid = f"script-{kind}-{i}-{int(time.time())}"
    req = {"token": tok, "id": rid, "kind": kind, "message": f"[scripted test {i}] The agent is stuck on Save report." if kind == "ask_help" else f"[scripted test {i}] {kind}",
           "schema": schemas[kind], "timeout_s": tmo, "deadline_ms": int(time.time() * 1000) + tmo * 1000, "meta": meta[kind]}
    s.sendall((json.dumps(req) + "\n").encode()); t0 = time.time()
    buf = b""
    while not buf.endswith(b"\n"):
        d = s.recv(4096)
        if not d: break
        buf += d
    print(f"{kind} #{i}: reply after {time.time()-t0:.1f}s:", buf.decode().strip()[:300])
    s.close()

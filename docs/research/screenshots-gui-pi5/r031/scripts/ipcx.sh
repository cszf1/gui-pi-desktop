# ipcx.sh : during run x1, a 25 s takeover (left unanswered), a help question that expires in the queue, then a shown help
cd /data/workspace
for i in $(seq 1 30); do pgrep -f "pi-coding-agent" >/dev/null && break; sleep 1; done; sleep 3
R=/data/workspace/runs/r031/x1
python3 ipc.py takeover 1 25 > $R/ipc_takeover.txt 2>&1 &
sleep 2
python3 ipc.py ask_help 1 8 > $R/ipc_help_expired.txt 2>&1
wait
python3 ipc.py ask_help 1 20 > $R/ipc_help_shown.txt 2>&1
cat $R/ipc_*.txt

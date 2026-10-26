# ipcx2.sh RUN : a 25 s takeover (unanswered), the same help question twice: first expires in the queue, then shown
cd /data/workspace
for i in $(seq 1 40); do pgrep -f "pi-coding-agent" >/dev/null && break; sleep 1; done; sleep 3
R=/data/workspace/runs/r031/$1
python3 ipc.py takeover 1 25 > $R/ipc_takeover.txt 2>&1 &
sleep 2
python3 ipc.py ask_help 1 8 > $R/ipc_help_expired.txt 2>&1
wait
python3 ipc.py ask_help 1 20 > $R/ipc_help_shown.txt 2>&1
cat $R/ipc_*.txt

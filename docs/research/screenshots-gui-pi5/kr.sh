. /data/workspace/env.sh
pkill -f gcr-prompter; pkill -f gnome-keyring-daemon; sleep 1
echo -n login > ~/.local/share/keyrings/default
printf '<REDACTED-test-keyring-password>' | setsid gnome-keyring-daemon --replace --unlock --components=secrets --daemonize > /tmp/gkr.env 2>&1 < /dev/stdin
sleep 2; cat /tmp/gkr.env; pgrep -a gnome-keyring
timeout 10 bash -c 'echo -n probevalue | secret-tool store --label=probe svc probe acct x && secret-tool lookup svc probe acct x && echo && secret-tool clear svc probe acct x' ; echo rc=$?
pgrep -a prompter

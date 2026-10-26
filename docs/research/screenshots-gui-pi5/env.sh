export DISPLAY=:1 XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=$(cat /tmp/dbus.addr)

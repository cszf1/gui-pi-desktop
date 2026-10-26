cd /data/workspace/rel/build-f2b6d6b-testhooks
dpkg-deb -I gui-pi-desktop_0.3.0_amd64.deb | grep -E "Depends|Version|Package"
dpkg-deb -c gui-pi-desktop_0.3.0_amd64.deb | awk '{print $6}' | grep -v "resources/" | head -20
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ./gui-pi-desktop_0.3.0_amd64.deb > /tmp/apt-gpd.log 2>&1; echo apt=$?; tail -3 /tmp/apt-gpd.log
dpkg -l | grep -E "gui-pi|webkit2gtk-4.1|gnome-keyring|at-spi2-core|libsecret" | awk '{print $2,$3}'
which gnome-keyring-daemon zenity xdotool python3; python3 -c 'import gi; print("gi ok")'

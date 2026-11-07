#!/bin/bash
# Runs INSIDE a fresh Debian trixie arm64 rootfs (proot, as fake root).
# Installs only what the Android runtime needs, then strips caches/docs.
# Expects /af (shared dir) to contain: box64 install tree (box64-install/),
# afbridge.c and run-big-buck.sh.
set -eux
export DEBIAN_FRONTEND=noninteractive
echo "nameserver 8.8.8.8" > /etc/resolv.conf
printf '127.0.0.1 localhost localhost.localdomain\n::1 localhost ip6-localhost ip6-loopback\n' > /etc/hosts
dpkg --add-architecture i386
apt-get update
apt-get install -y --no-install-recommends \
    xvfb xauth x11-xserver-utils \
    libgl1 libglx-mesa0 libgl1-mesa-dri libglu1-mesa libglut3.12 \
    libxcomposite1 libxinerama1 libxcursor1 libxfixes3 libxrandr2 libxrender1 \
    libfreetype6 libfontconfig1 fontconfig-config fonts-dejavu-core \
    libsdl1.2debian libpulse0 pulseaudio libasound2-plugins libx11-6 libxext6 libxtst6 libxi6 libxxf86vm1 \
    libglut3.12:i386 libgcc-s1:i386 libstdc++6:i386 \
    gcc libc6-dev libx11-dev libxext-dev libxtst-dev libpulse-dev
# Bridge between Xvfb and the Android app.
gcc -O2 -Wall -o /usr/local/bin/afbridge /af/afbridge.c -lX11 -lXext -lXtst -lpthread
gcc -O2 -Wall -o /usr/local/bin/afaudio /af/afaudio.c -lpulse-simple -lpulse
# box64 (built with BOX32 and the Banana pepper section-less ELF patch).
cp -a /af/box64-install/. /
strip --strip-debug /usr/local/bin/box64
apt-get purge -y gcc libc6-dev libx11-dev libxext-dev libxtst-dev libpulse-dev binutils
apt-get autoremove -y --purge
mkdir -p /opt/arcade
install -m 755 /af/run-big-buck.sh /opt/arcade/run-big-buck.sh
# Trim the image.
apt-get clean
rm -rf /var/lib/apt/lists/* /var/cache/* /usr/share/doc/* /usr/share/man/* \
       /usr/share/info/* /usr/share/lintian /usr/share/locale/* /tmp/* /root/box64
/bin/bash /af/trim-rootfs-guest.sh
echo ROOTFS_READY

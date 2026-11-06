#!/system/bin/sh
# Enter the Debian rootfs via proot (dev loop under adb shell).
AF=/data/local/tmp/af
R=$AF/rootfs/debian-trixie-aarch64
export LD_LIBRARY_PATH=$AF/bin PROOT_LOADER=$AF/bin/loader PROOT_TMP_DIR=$AF/tmp
unset LD_PRELOAD
exec $AF/bin/proot --kill-on-exit --link2symlink -0 -r $R \
  -b /dev -b /proc -b /sys -b $AF/tmp:/dev/shm -b $AF/tmp:/tmp \
  -b /data/local/tmp/af/share:/af -w /root \
  /usr/bin/env -i HOME=/root TERM=xterm PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin LANG=C.UTF-8 "$@"

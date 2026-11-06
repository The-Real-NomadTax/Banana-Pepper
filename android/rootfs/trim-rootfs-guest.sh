#!/bin/bash
# Runs INSIDE the release rootfs after build-rootfs-guest.sh: removes files the
# Android runtime never uses. box32 wraps the native arm64 GL/X11 libraries, so
# the i386 Mesa/LLVM pulled in as package dependencies are dead weight.
set -eux
[ -f /af/box64-install/usr/local/bin/box64 ] && cp /af/box64-install/usr/local/bin/box64 /usr/local/bin/box64
rm -rf /usr/lib/i386-linux-gnu/dri /usr/lib/i386-linux-gnu/libLLVM* /usr/lib/i386-linux-gnu/libgallium* \
       /usr/lib/i386-linux-gnu/libz3* /usr/lib/i386-linux-gnu/libicu* /usr/lib/i386-linux-gnu/libxml2* \
       /usr/lib/i386-linux-gnu/libSPIRV* /usr/lib/i386-linux-gnu/libvulkan* \
       /usr/lib/box64-x86_64-linux-gnu /usr/libexec/gcc /usr/lib/gcc /usr/share/i18n /usr/include \
       /usr/local/bin/box64-bash /usr/local/bin/box64-python /usr/local/bin/box64-configurator
echo TRIM_DONE

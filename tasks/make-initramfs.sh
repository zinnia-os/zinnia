#!/bin/bash

set -e

BUILD_DIR="$XBSTRAP_BUILD_ROOT"
INITRAMFS_DIR="$BUILD_DIR/initramfs"
INITRAMFS_PATH="$(realpath $1)"

# Make sure the initramfs doesn't already exist.
rm -f $INITRAMFS_PATH
mkdir -p $INITRAMFS_DIR

# `tar` operates on the CWD.
cd $INITRAMFS_DIR

mkdir -p usr/bin
mkdir -p usr/lib

# Compatibility symlinks
ln -fs usr/lib lib
ln -fs usr/bin bin

# Create the initramfs with the following files.
FILES=(
    # Compatibility symlinks
    bin
    lib
    # libc and loader
    usr/lib/ld.so
    usr/lib/libc.so
    usr/lib/libpthread.so
    usr/lib/libm.so
    # Servers
    # usr/bin/initd
    # usr/bin/posixd
)
echo "Installing:" ${FILES[@]}

for file in ${FILES[@]}; do
    cp -rP "$XBSTRAP_SYSROOT_DIR/$file" "$INITRAMFS_DIR/$file"
done

tar --format=ustar --owner 0 --group 0 -cf $INITRAMFS_PATH "${FILES[@]}"

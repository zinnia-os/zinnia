#!/bin/bash

set -e

SYSTEM_ROOT="$1"
OUTPUT_IMAGE="$2"

# Setup loop device
LOOPDEV=$(sudo losetup --find --show --partscan "$OUTPUT_IMAGE")
ESP_PART="${LOOPDEV}p1"
ROOT_PART="${LOOPDEV}p2"

# Create temporary directories
tmpdir=$(sudo mktemp -d)
sudo mkdir -p "$tmpdir/root"
# Mount root partition
sudo mount "$ROOT_PART" "$tmpdir/root"
# Mount the ESP inside /boot
sudo mkdir -p "$tmpdir/root/boot"
# FAT has no UIDs, so simulate a user, otherwise cp will complain.
sudo mount -o uid=1000,gid=1000 "$ESP_PART" "$tmpdir/root/boot"

# Copy system root
sudo rsync -avr --checksum "$SYSTEM_ROOT/" "$tmpdir/root"

# Unmount and detach
sudo umount "$tmpdir/root/boot"
sudo umount "$tmpdir/root"
sudo losetup -d "$LOOPDEV"
sudo rm -rf "$tmpdir"

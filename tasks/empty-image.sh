#!/bin/bash

set -e

OUTPUT_IMAGE="$1"
IMAGE_SIZE="$2"
ESP_SIZE="$3"

# Create an empty disk image
truncate -s $IMAGE_SIZE "$OUTPUT_IMAGE"

# Setup loop device
LOOPDEV=$(sudo losetup --find --show "$OUTPUT_IMAGE")
ESP_PART="${LOOPDEV}p1"
ROOT_PART="${LOOPDEV}p2"

# Create partitions
sudo parted -s "$LOOPDEV" mklabel gpt
sudo parted -s "$LOOPDEV" mkpart ESP fat32 1MiB ${ESP_SIZE}
sudo parted -s "$LOOPDEV" set 1 esp on
sudo parted -s "$LOOPDEV" mkpart ROOT ext4 ${ESP_SIZE} 100%

# Format partitions
sudo mkfs.vfat -F 32 "$ESP_PART"
sudo mkfs.ext4 "$ROOT_PART"

# Detach
sudo losetup -d "$LOOPDEV"

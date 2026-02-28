#!/bin/python3

# Copyright © 2026, Julian Scheffers
# SPDX-License-Identifier: MIT

import argparse, subprocess, tempfile, os

Block =  512
Kb    = 1024
MiB   = 1024 * Kb
GiB   = 1024 * MiB

class Partition:
    def __init__(self, name: str, typeid: str, blob: str, size: int, offset: int = 0):
        self.name   = name
        self.typeid = typeid
        self.blob   = blob
        self.size   = size
        self.offset = offset

def div_ceil(a: int, b: int) -> int:
    return (a + b - 1) // b

def mount_or_format(mkfs: str, device: str, mountpoint: str, args: list[str] = []):
    try:
        subprocess.check_call(['sudo', 'mount'] + args + [device, mountpoint])
    except:
        subprocess.check_call(['sudo', mkfs, device])
        subprocess.check_call(['sudo', 'mount'] + args + [device, mountpoint])



parser = argparse.ArgumentParser()
parser.add_argument("sysroot", help="Filsystem root")
parser.add_argument("output", help="Output image file")
parser.add_argument("--no-sudo", help="Do a slower but sudo-less image creation")

args=parser.parse_args()
sysroot: str = args.sysroot
output: str = args.output
no_sudo: bool = args.no_sudo



# Calculate image size.
image_size   =   4 * GiB
gpt_overhead =  67 * Block # 34 at the start, 33 at the end
esp_size     = 128 * MiB
root_size    = image_size - esp_size - gpt_overhead



tmpdir = tempfile.mkdtemp()
try:
    parts = [
        Partition('EFI part',  '0x0700', f'{tmpdir}/boot.bin', esp_size),
        Partition('Root part', '0x8300', f'{tmpdir}/root.bin', root_size)
    ]
    
    # Create disk image.
    if no_sudo:
        subprocess.check_call(['truncate', '-s', '0', output])
    subprocess.check_call(['truncate', '-s', str(image_size), output])
    sgdisk = ['sgdisk', '-o', '-a', '1']
    if not no_sudo:
        sgdisk = ['sudo'] + sgdisk
    offset = 34*Block
    print(f'Creating image from {len(parts)} partitions:')
    for i in range(len(parts)):
        part = parts[i]
        part.offset = offset
        sgdisk += [
            f'--new={i+1}:{part.offset//Block}:{div_ceil(part.offset+part.size, Block)-1}',
            f'--change-name={i+1}:{part.name}',
            f'--typecode={i+1}:{part.typeid}'
        ]
        print(f'    "{part.name}", type {part.typeid}, size {part.size}, offset {part.offset}')
        offset += div_ceil(part.size, Block) * Block
    print()
    sgdisk += [output]
    subprocess.check_call(sgdisk) # sgdisk -a 1 $parts $output
    
    if no_sudo:
        # Create root filesystem image without sudo (slow iteration).
        subprocess.check_call(['mkdir', '-p', f'{sysroot}/boot'])
        subprocess.check_call(['mv', f'{sysroot}/boot', f'{tmpdir}/boot']) # shuffle $sysroot/boot out of the way while making the ext4 image
        try:
            subprocess.check_call(['mkdir', f'{sysroot}/boot'])
            subprocess.check_call(['truncate', '-s', str(root_size), f'{tmpdir}/root.bin'])
            subprocess.check_call(['mkfs.ext2', '-i', '16384', '-d', sysroot, f'{tmpdir}/root.bin']) # actually create the ext4 image
        finally:
            subprocess.call(['rmdir', f'{sysroot}/boot'])
            subprocess.call(['mv', f'{tmpdir}/boot', f'{sysroot}/boot']) # restore $sysroot/boot to former location
        
        # Create EFI filesystem image.
        subprocess.check_call(['truncate', '-s', str(esp_size), f'{tmpdir}/boot.bin'])
        subprocess.check_call(['mformat', '-i', f'{tmpdir}/boot.bin', '-v', 'Zinnia'])
        files = subprocess.check_output(['find', f'{sysroot}/boot/', '-mindepth', '1', '-maxdepth', '1']).split()
        subprocess.check_call(['mcopy', '-s', '-i', f'{tmpdir}/boot.bin'] + files + ['::/'])
        
        for part in parts:
            subprocess.check_call(['dd', f'bs={Block}', f'if={part.blob}', f'of={output}', f'seek={part.offset//Block}', 'conv=notrunc,sparse'])
    
    else:
        # Create/update root filesystem image with sudo (fast iteration).
        loopdev = subprocess.check_output(['sudo', 'losetup', '--find', '--show', '--partscan', output]).decode('utf-8').strip()
        try:
            mount_or_format('mkfs.ext2', f'{loopdev}p2', tmpdir)
            subprocess.check_call(['sudo', 'mkdir', '-p', f'{tmpdir}/boot'])
            mount_or_format('mkfs.vfat', f'{loopdev}p1', f'{tmpdir}/boot', ['-o', f'uid={os.geteuid()},gid={os.getegid()}'])
            
            subprocess.check_call(['sudo', 'rsync', '-avr', '--checksum', f'{sysroot}/', tmpdir])
        finally:
            subprocess.call(['sudo', 'umount', f'{tmpdir}/boot'])
            subprocess.call(['sudo', 'umount', tmpdir])
            subprocess.call(['sudo', 'losetup', '-d', loopdev])
    
finally:
    subprocess.check_call(['rm', '-rf', tmpdir])

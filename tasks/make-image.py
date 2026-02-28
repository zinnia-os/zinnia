#!/bin/python3

# Copyright © 2026, Julian Scheffers
# SPDX-License-Identifier: MIT

import argparse, subprocess, tempfile

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



parser = argparse.ArgumentParser()
parser.add_argument("sysroot", help="Filsystem root")
parser.add_argument("output", help="Output image file")

args=parser.parse_args()
sysroot: str = args.sysroot
output: str = args.output



# Calculate image size.
image_size   =   4 * GiB
gpt_overhead =  67 * Block # 34 at the start, 33 at the end
esp_size     = 128 * MiB
root_size    = image_size - esp_size - gpt_overhead



tmpdir = tempfile.mkdtemp()
try:
    # Create root filesystem image.
    subprocess.check_call(['mkdir', '-p', f'{sysroot}/boot'])
    subprocess.check_call(['mv', f'{sysroot}/boot', f'{tmpdir}/boot']) # shuffle $sysroot/boot out of the way while making the ext4 image
    try:
        subprocess.check_call(['mkdir', f'{sysroot}/boot'])
        subprocess.check_call(['truncate', '-s', str(root_size), f'{tmpdir}/root.bin'])
        subprocess.check_call(['mkfs.ext2', '-i', '16384', '-d', sysroot, f'{tmpdir}/root.bin']) # actually create the ext4 image
    finally:
        subprocess.check_call(['rmdir', f'{sysroot}/boot'])
        subprocess.check_call(['mv', f'{tmpdir}/boot', f'{sysroot}/boot']) # restore $sysroot/boot to former location

    # Create EFI filesystem image.
    subprocess.check_call(['truncate', '-s', str(esp_size), f'{tmpdir}/boot.bin'])
    subprocess.check_call(['mformat', '-i', f'{tmpdir}/boot.bin', '-v', 'Zinnia'])
    files = subprocess.check_output(['find', f'{sysroot}/boot/', '-mindepth', '1', '-maxdepth', '1']).split()
    subprocess.check_call(['mcopy', '-s', '-i', f'{tmpdir}/boot.bin'] + files + ['::/'])
    
    parts = [
        Partition('EFI part',  '0x0700', f'{tmpdir}/boot.bin', esp_size),
        Partition('Root part', '0x8300', f'{tmpdir}/root.bin', root_size)
    ]
    
    # Create disk image.
    subprocess.check_call(['truncate', '-s', '0', output])
    subprocess.check_call(['truncate', '-s', str(image_size), output])
    sgdisk = ['sgdisk', '-a', '1']
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
    for part in parts:
        subprocess.check_call(['dd', f'bs={Block}', f'if={part.blob}', f'of={output}', f'seek={part.offset//Block}', 'conv=notrunc,sparse'])
    
finally:
    subprocess.check_call(['rm', '-rf', tmpdir])

#!/usr/bin/env python3
# SPDX-License-Identifier: BSD-3-Clause

# Small script to print all options available inside a built kernel.
# fmt: off
import sys

from elftools.elf.elffile import ELFFile

SIZEOF_PTR = 8
SIZEOF_STRUCT = 40
ENDIAN = "little"


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Provide a path to the kernel binary to use")
        sys.exit(1)

    with open(sys.argv[1], "rb") as elffile:
        file = ELFFile(elffile)
        section = file.get_section_by_name(".cmdline")
        if section == None:
            sys.exit(1)

        cmdline_data = section.data()
        num_options = int(section.data_size / SIZEOF_PTR)

        print(f"Kernel contains {num_options} options:")

        for i in range(num_options):
            # Read "struct cmdline_option*"
            addr = int.from_bytes(cmdline_data[(i * SIZEOF_PTR) :][:SIZEOF_PTR], byteorder=ENDIAN)
            elffile.seek(next(file.address_offsets(start=addr)))
            option = elffile.read(SIZEOF_STRUCT)

            # Name
            name = ""
            name_ptr = int.from_bytes(option[0:][:SIZEOF_PTR], byteorder=ENDIAN)
            elffile.seek(next(file.address_offsets(start=name_ptr)))
            while True:
                b = int.from_bytes(elffile.read(1))
                if b == 0:
                    break
                name += chr(b)

            # Description
            desc = ""
            desc_ptr = int.from_bytes(option[8:][:SIZEOF_PTR], byteorder=ENDIAN)
            elffile.seek(next(file.address_offsets(start=desc_ptr)))
            while True:
                b = int.from_bytes(elffile.read(1))
                if b == 0:
                    break
                desc += chr(b)

            # Type
            kind_str = "unknown"
            kind = int.from_bytes(option[16:][:4], byteorder=ENDIAN)
            match kind:
                case 0:
                    kind_str = "string"
                case 1:
                    kind_str = "bool"
                case 2:
                    kind_str = "signed integer"
                case 3:
                    kind_str = "unsigned integer"

            print(f"- {name}:\t{desc} ({kind_str})")

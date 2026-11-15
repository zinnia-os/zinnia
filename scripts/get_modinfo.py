#!/usr/bin/env python3
# SPDX-License-Identifier: BSD-3-Clause

# Small script to print all module information available.
# fmt: off
import struct
import sys

from elftools.elf.dynamic import DynamicSegment
from elftools.elf.elffile import ELFFile
from elftools.elf.relocation import RelrRelocationTable

SIZEOF_PTR = 8
SIZEOF_STRUCT = 40
ENDIAN = "little"

MATCH_SEGMENT = 0x60000007
TABLE_FORMAT = "QQQ"
PTR_SIZE = 8


def format_id(value):
    return "****" if value == 0xFFFF else f"{value:04x}"


def format_class(value):
    return "**" if value == 0xFF else f"{value:02x}"


def format_ids(module, addr):
    first, second, cls, subcls, iface = struct.unpack(
        module.endian + "HHBBB", module.read(addr, 7)
    )
    ids = f"{format_id(first)}:{format_id(second)}"
    classes = f"{format_class(cls)}:{format_class(subcls)}:{format_class(iface)}"
    return f"<id = {ids}, class = {classes}>"


def format_fdt(module, addr):
    compatible = module.read_string(module.read_pointer(addr))
    if compatible is None:
        return "<compatible = *>"
    return f'<compatible = "{compatible}">'


def format_acpi(module, addr):
    hid = module.read_string(module.read_pointer(addr))
    if hid is None:
        return "<hid = *>"
    return f'<hid = "{hid}">'


KINDS = {
    "pci": {"size": 16, "format": format_ids},
    "usb": {"size": 16, "format": format_ids},
    "fdt": {"size": 16, "format": format_fdt},
    "acpi": {"size": 16, "format": format_acpi},
}


class Module:
    def __init__(self, stream):
        self.stream = stream
        self.file = ELFFile(stream)
        self.byteorder = "little" if self.file.little_endian else "big"
        self.endian = "<" if self.file.little_endian else ">"
        self.addends = {}
        for segment in self.file.iter_segments():
            if isinstance(segment, DynamicSegment):
                for table in segment.get_relocation_tables().values():
                    if not isinstance(table, RelrRelocationTable) and table.is_RELA():
                        for reloc in table.iter_relocations():
                            self.addends[reloc["r_offset"]] = reloc["r_addend"]

    def match_region(self):
        for segment in self.file.iter_segments():
            if segment["p_type"] == MATCH_SEGMENT:
                return segment["p_vaddr"], segment.data()
        return 0, b""

    def read(self, addr, size):
        self.stream.seek(next(self.file.address_offsets(addr, size)))
        return self.stream.read(size)

    def pointer(self, raw, addr):
        return raw if raw != 0 else self.addends.get(addr, 0)

    def read_pointer(self, addr):
        return self.pointer(
            int.from_bytes(self.read(addr, PTR_SIZE), self.byteorder), addr
        )

    def read_string(self, addr):
        if addr == 0:
            return None
        self.stream.seek(next(self.file.address_offsets(addr)))
        raw = b""
        while (byte := self.stream.read(1)) not in (b"", b"\0"):
            raw += byte
        return raw.decode(errors="replace")


def parse_matches(module):
    base, data = module.match_region()
    table_size = struct.calcsize(module.endian + TABLE_FORMAT)

    tables = []
    offset = 0
    while offset + table_size <= len(data):
        addr = base + offset
        raw_name, raw_entries, count = struct.unpack_from(
            module.endian + TABLE_FORMAT, data, offset
        )
        name_addr = module.pointer(raw_name, addr)
        if name_addr == 0:
            offset += PTR_SIZE
            continue
        offset += table_size

        name = module.read_string(name_addr)
        kind = KINDS.get(name)
        if kind is None:
            tables.append((name, count, None))
            continue

        entries_addr = module.pointer(raw_entries, addr + PTR_SIZE)
        entries = [
            kind["format"](module, entries_addr + i * kind["size"])
            for i in range(count)
        ]
        tables.append((name, count, entries))
    return tables


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Provide a path to the kernel binary to use")
        sys.exit(1)

    with open(sys.argv[1], "rb") as elffile:
        module = Module(elffile)
        file = module.file

        version = "?"
        author = "?"
        desc = "?"
        license = "?"

        for segment in file.iter_segments():
            match segment["p_type"]:
                case 0x60000001: version = segment.data()[:-1].decode()
                case 0x60000002: author = segment.data()[:-1].decode()
                case 0x60000003: desc = segment.data()[:-1].decode()
                case 0x60000004: license = segment.data()[:-1].decode()

        print(f"Module info for \"{sys.argv[1]}\"")
        print(f"Description:\t{desc}")
        print(f"Version:\t{version}")
        print(f"Author(s):\t{author}")
        print(f"License:\t{license}")

        tables = parse_matches(module)
        if not tables:
            print("Devices:\tnone")
        else:
            print("Devices:")
        for name, count, entries in tables:
            rules = f"{count} rule{'' if count == 1 else 's'}"
            if entries is None:
                print(f"  {name} ({rules}, unknown type)")
                continue
            print(f"  {name} ({rules})")
            for j, entry in enumerate(entries):
                print(f"    [{j}] = {entry}")

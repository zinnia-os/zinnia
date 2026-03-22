# Expected machine state after `_start`

Upon entry in `main()`, Zinnia expects the following machine state:

## System state
- Interrupts should be turned off.
- All AP cores should be disabled and waiting to be initialized.

## Memory layout

In virtual memory, the following mapping should exist:

| Segment | Virtual address   | Length in bytes                     | Permissions |
| ------- | ----------------- | ----------------------------------- | ----------- |
| text    | `LD_TEXT_START`   | `LD_TEXT_END` - `LD_TEXT_START`     | `r-x`       |
| rodata  | `LD_RODATA_START` | `LD_RODATA_END` - `LD_RODATA_START` | `r--`       |
| data    | `LD_DATA_START`   | `LD_DATA_END` - `LD_DATA_START`     | `rw-`       |

The kernel should be loaded physically contiguously, there is no specific memory layout.

The `data` segment needs at least `0x1000_0000` bytes of distance from the end of the higher half region.
This means that for KASLR, the highest address for `LD_KERNEL_END` can be `0xffff_ffff_efff_ffff`.

Additionally, there should be an HHDM mapping from physical address `0` up until the highest usable memory address.
The virtual start address depends on the paging mode used, e.g. on 48-bit paging, the base address should be
`0xffff_8000_0000_0000` or higher and it should encompass all usable physical memory.

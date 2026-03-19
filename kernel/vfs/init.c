#include <kernel/elf.h>
#include <kernel/exec.h>
#include <kernel/init.h>
#include <kernel/vfs.h>

[[__init]]
void vfs_init() {
    elf_init();
}

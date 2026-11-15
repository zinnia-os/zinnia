// SPDX-License-Identifier: BSD-3-Clause

#include <stdio.h>
#include <string.h>
#include <sys/mount.h>

int main(int argc, char** argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <fstype> <target> [source]\n", argv[0]);
        return 2;
    }

    const char* fstype = argv[1];
    const char* target = argv[2];
    const char* source = argc >= 4 ? argv[3] : NULL;

    if (mount(fstype, target, 0, (void*)source) != 0) {
        perror("mount");
        return 1;
    }

    return 0;
}

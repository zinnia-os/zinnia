// SPDX-License-Identifier: BSD-3-Clause

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/module.h>

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <module-path> [cmdline]\n", argv[0]);
        return 2;
    }

    const char* path = argv[1];
    const char* cmdline = argc >= 3 ? argv[2] : NULL;

    if (insertmod(path, cmdline) != 0) {
        perror("insertmod");
        return 1;
    }

    return 0;
}

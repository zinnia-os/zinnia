// SPDX-License-Identifier: BSD-3-Clause

#include <stdio.h>
#include <sys/mount.h>

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <target>\n", argv[0]);
        return 2;
    }

    if (unmount(argv[1], 0) != 0) {
        perror("umount");
        return 1;
    }

    return 0;
}

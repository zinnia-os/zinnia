// SPDX-License-Identifier: BSD-3-Clause

#define _GNU_SOURCE

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <unistd.h>

int main(int argc, char** argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <tty> <command> [args...]\n", argv[0]);
        return 2;
    }

    const char* tty = argv[1];

    if (setsid() < 0 && getpid() != getsid(0)) {
        perror("setsid");
        return 1;
    }

    int fd = open(tty, O_RDWR);
    if (fd < 0) {
        perror(tty);
        return 1;
    }

    if (ioctl(fd, TIOCSCTTY, 0) != 0)
        perror("TIOCSCTTY");

    if (dup2(fd, STDIN_FILENO) < 0 || dup2(fd, STDOUT_FILENO) < 0 || dup2(fd, STDERR_FILENO) < 0) {
        perror("dup2");
        return 1;
    }

    if (fd > STDERR_FILENO)
        close(fd);

    execvp(argv[2], &argv[2]);
    perror(argv[2]);
    return 1;
}

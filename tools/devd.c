// SPDX-License-Identifier: BSD-3-Clause

#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#define DEVCTL_PATH "/dev/devctl"
#define SOCKET_PATH "/var/run/devd.pipe"
#define MAX_CLIENTS 32
#define DEVCTL_BUF  4096
#define LINE_BUF    1024

static int clients[MAX_CLIENTS];
static int nclients = 0;

// Accumulates a partial line read from /dev/devctl across reads.
static char line[LINE_BUF];
static size_t line_len = 0;

static void add_client(int fd) {
    if (nclients >= MAX_CLIENTS) {
        close(fd);
        return;
    }
    clients[nclients++] = fd;
}

static void remove_client(int idx) {
    close(clients[idx]);
    clients[idx] = clients[--nclients];
}

// Broadcast a single notification line.
static void broadcast(const char* buf, size_t len) {
    for (int i = 0; i < nclients;) {
        ssize_t off = 0;
        int dead = 0;
        while (off < (ssize_t)len) {
            ssize_t n = write(clients[i], buf + off, len - off);
            if (n < 0) {
                if (errno == EINTR)
                    continue;
                dead = 1;
                break;
            }
            off += n;
        }
        if (dead)
            remove_client(i);
        else
            i++;
    }
}

// Feed raw bytes from /dev/devctl.
static void feed(const char* data, size_t len) {
    for (size_t i = 0; i < len; i++) {
        char c = data[i];
        if (line_len < sizeof(line) - 1)
            line[line_len++] = c;
        if (c == '\n') {
            broadcast(line, line_len);
            line_len = 0;
        }
    }
    // Drop a line without terminator if it gets too big.
    if (line_len >= sizeof(line) - 1)
        line_len = 0;
}

static int open_devctl(void) {
    int fd = open(DEVCTL_PATH, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        perror("devd: open " DEVCTL_PATH);
    return fd;
}

static int make_listener(void) {
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        perror("devd: socket");
        return -1;
    }

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, SOCKET_PATH, sizeof(addr.sun_path) - 1);

    unlink(SOCKET_PATH);
    if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("devd: bind " SOCKET_PATH);
        close(fd);
        return -1;
    }
    chmod(SOCKET_PATH, 0666);

    if (listen(fd, 8) < 0) {
        perror("devd: listen");
        close(fd);
        return -1;
    }
    return fd;
}

int main(void) {
    // A client going away must not kill us.
    signal(SIGPIPE, SIG_IGN);

    int devctl = open_devctl();
    if (devctl < 0)
        return 1;

    int listener = make_listener();
    if (listener < 0)
        return 1;

    printf("devd: listening on %s, forwarding from %s\n", SOCKET_PATH, DEVCTL_PATH);
    fflush(stdout);

    for (;;) {
        struct pollfd pfds[2 + MAX_CLIENTS];
        pfds[0].fd = devctl;
        pfds[0].events = POLLIN;
        pfds[1].fd = listener;
        pfds[1].events = POLLIN;
        for (int i = 0; i < nclients; i++) {
            pfds[2 + i].fd = clients[i];
            pfds[2 + i].events = 0; // Only watch for hangup.
        }

        int n = poll(pfds, 2 + nclients, -1);
        if (n < 0) {
            if (errno == EINTR)
                continue;
            perror("devd: poll");
            break;
        }

        // New notifications from the kernel.
        if (pfds[0].revents & POLLIN) {
            char buf[DEVCTL_BUF];
            ssize_t r = read(devctl, buf, sizeof(buf));
            if (r > 0) {
                feed(buf, (size_t)r);
            } else if (r == 0 || (r < 0 && errno != EINTR && errno != EAGAIN)) {
                fprintf(stderr, "devd: %s closed, exiting\n", DEVCTL_PATH);
                break;
            }
        }

        // New client connections.
        if (pfds[1].revents & POLLIN) {
            int c = accept(listener, NULL, NULL);
            if (c >= 0) {
                fcntl(c, F_SETFD, FD_CLOEXEC);
                add_client(c);
            }
        }

        // Reap clients that hung up.
        for (int i = nclients - 1; i >= 0; i--) {
            if (pfds[2 + i].revents & (POLLHUP | POLLERR | POLLNVAL))
                remove_client(i);
        }
    }

    close(devctl);
    close(listener);
    unlink(SOCKET_PATH);
    return 1;
}

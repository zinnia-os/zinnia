// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/iovec.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/usercopy.h>
#include <net/socket.h>
#include <syscalls/private.h>
#include <stddef.h>
#include <string.h>
#include <zinnia/socket.h>

constexpr size_t SOCKET_CONTROL_MAX = 0x10000;
constexpr size_t SOCKET_OPTION_MAX = 256;
constexpr enum sock_type SOCK_FLAGS = SOCK_NONBLOCK | SOCK_CLOEXEC | SOCK_CLOFORK;

static errno_t get_socket(int fd, struct vfs_file** out_file, struct socket** out) {
    struct vfs_file* file = process_fd_get(self(), fd);
    if (!file)
        return EBADF;

    struct socket* sock = socket_from_file(file);
    if (!sock) {
        vfs_file_unref(file);
        return ENOTSOCK;
    }

    *out_file = file;
    *out = sock;
    return 0;
}

static errno_t install(struct socket* sock, enum sock_type flags, int* out_fd) {
    struct process* proc = self();
    struct vfs_file* file;
    errno_t status = socket_open_file(sock, (flags & SOCK_NONBLOCK) ? O_NONBLOCK : 0, &proc->identity, &file);
    if (status)
        return status;

    status = process_fd_alloc(proc, file, flags & SOCK_CLOEXEC, out_fd);
    vfs_file_unref(file);
    return status;
}

static errno_t copy_sockaddr(const __user void* addr, size_t len, struct __sockaddr_storage* out) {
    if (!addr || !len || len > sizeof(*out))
        return EINVAL;
    memset(out, 0, sizeof(*out));
    return usercopy_read(out, addr, len) ? 0 : EFAULT;
}

static errno_t write_name(struct socket* sock, bool peer, __user void* addr, __user __socklen_t* len) {
    auto op = peer ? sock->ops->getpeername : sock->ops->getsockname;
    if (!op)
        return EOPNOTSUPP;

    struct __sockaddr_storage kaddr = {};
    size_t klen = 0;
    errno_t status = op(sock, &kaddr, &klen);
    if (status || !addr || !len)
        return status;

    // Copy the address out truncated to the user buffer, and report its full length.
    __socklen_t max;
    if (!usercopy_read(&max, len, sizeof(max)))
        return EFAULT;
    const size_t n = MIN((size_t)max, klen);
    if (n && !usercopy_write(addr, &kaddr, n))
        return EFAULT;
    const __socklen_t actual = klen;
    return usercopy_write(len, &actual, sizeof(actual)) ? 0 : EFAULT;
}

static void close_rights(const void* control, size_t len) {
    const size_t header = ROUND_UP(sizeof(struct __cmsghdr), sizeof(size_t));
    const uint8_t* buf = control;

    size_t off = 0;
    while (off < len && len - off >= sizeof(struct __cmsghdr)) {
        struct __cmsghdr hdr;
        memcpy(&hdr, buf + off, sizeof(hdr));
        if (hdr.cmsg_len < sizeof(hdr) || hdr.cmsg_len > len - off)
            break;

        if (hdr.cmsg_level == __SOL_SOCKET && hdr.cmsg_type == __SCM_RIGHTS) {
            const size_t count = hdr.cmsg_len > header ? (hdr.cmsg_len - header) / sizeof(int) : 0;
            for (size_t i = 0; i < count; i++) {
                int fd;
                memcpy(&fd, buf + off + header + i * sizeof(int), sizeof(fd));
                process_fd_close(self(), fd);
            }
        }

        off += ROUND_UP(hdr.cmsg_len, sizeof(size_t));
    }
}

static size_t msg_iov_size(const struct __msghdr* msg) {
    return MAX((size_t)msg->msg_iovlen, 1) * sizeof(iovec_t);
}

static errno_t read_msghdr(
    const __user struct __msghdr* hdr,
    struct __msghdr* out,
    iovec_t** out_buf,
    iovec_iter_t* out_iter
) {
    if (!usercopy_read(out, hdr, sizeof(*out)))
        return EFAULT;
    if (out->msg_iovlen < 0 || (size_t)out->msg_iovlen > IOV_MAX)
        return EINVAL;

    iovec_t* buf = mem_alloc(msg_iov_size(out), 0);
    if (!buf)
        return ENOMEM;

    errno_t status = copy_iovecs(out->msg_iov, out->msg_iovlen, buf, out_iter);
    if (status) {
        mem_free(buf, msg_iov_size(out));
        return status;
    }

    *out_buf = buf;
    return 0;
}

struct syscall_ret sys_socket(int family, uint32_t type, int protocol) {
    if (family < 0 || family > UINT16_MAX)
        return result(EAFNOSUPPORT, 0);

    struct socket* sock;
    errno_t status = socket_create(family, type & ~SOCK_FLAGS, protocol, &sock);
    if (status)
        return result(status, 0);

    int fd;
    status = install(sock, type & SOCK_FLAGS, &fd);
    socket_unref(sock);
    return result(status, fd);
}

struct syscall_ret sys_socketpair(int family, uint32_t type, int protocol) {
    if (family < 0 || family > UINT16_MAX)
        return result(EAFNOSUPPORT, 0);

    struct socket* a;
    struct socket* b;
    errno_t status = socket_create_pair(family, type & ~SOCK_FLAGS, protocol, &a, &b);
    if (status)
        return result(status, 0);

    int fds[2] = {-1, -1};
    status = install(a, type & SOCK_FLAGS, &fds[0]);
    if (!status)
        status = install(b, type & SOCK_FLAGS, &fds[1]);

    if (status) {
        for (size_t i = 0; i < 2; i++) {
            if (fds[i] >= 0)
                process_fd_close(self(), fds[i]);
        }
        socket_release(a);
        socket_release(b);
        return result(status, 0);
    }

    socket_unref(a);
    socket_unref(b);
    return result(0, (uintptr_t)(uint32_t)fds[0] | ((uintptr_t)(uint32_t)fds[1] << 32));
}

struct syscall_ret sys_bind(int fd, const __user void* addr, size_t len) {
    struct __sockaddr_storage kaddr;
    errno_t status = copy_sockaddr(addr, len, &kaddr);
    if (status)
        return result(status, 0);

    struct vfs_file* file;
    struct socket* sock;
    status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    status = sock->ops->bind ? sock->ops->bind(sock, &kaddr, len) : EOPNOTSUPP;
    vfs_file_unref(file);
    return result(status, 0);
}

struct syscall_ret sys_listen(int fd, int backlog) {
    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    status = sock->ops->listen ? sock->ops->listen(sock, backlog) : EOPNOTSUPP;
    vfs_file_unref(file);
    return result(status, 0);
}

struct syscall_ret sys_accept(
    int fd,
    __user int* newfd,
    __user void* addr,
    __user __socklen_t* addr_len,
    uint32_t flags
) {
    if (flags & ~SOCK_FLAGS)
        return result(EINVAL, 0);

    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    struct socket* conn;
    status = sock->ops->accept ? sock->ops->accept(sock, vfs_file_nonblocking(file), &conn) : EOPNOTSUPP;
    vfs_file_unref(file);
    if (status)
        return result(status, 0);

    int kfd;
    if (addr && addr_len)
        status = write_name(conn, true, addr, addr_len);
    if (!status)
        status = install(conn, flags, &kfd);
    if (status) {
        socket_release(conn);
        return result(status, 0);
    }
    socket_unref(conn);

    if (!usercopy_write(newfd, &kfd, sizeof(kfd))) {
        process_fd_close(self(), kfd);
        return result(EFAULT, 0);
    }
    return result(0, 0);
}

struct syscall_ret sys_connect(int fd, const __user void* addr, size_t len) {
    struct __sockaddr_storage kaddr;
    errno_t status = copy_sockaddr(addr, len, &kaddr);
    if (status)
        return result(status, 0);

    struct vfs_file* file;
    struct socket* sock;
    status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    if (len >= sizeof(__sa_family_t) && kaddr.ss_family == AF_UNSPEC)
        status = sock->ops->disconnect ? sock->ops->disconnect(sock) : EOPNOTSUPP;
    else
        status = sock->ops->connect ? sock->ops->connect(sock, &kaddr, len, vfs_file_nonblocking(file)) : EOPNOTSUPP;
    vfs_file_unref(file);
    return result(status, 0);
}

struct syscall_ret sys_sendmsg(int fd, const __user struct __msghdr* hdr, uint32_t flags) {
    struct __msghdr msg;
    iovec_t* iovecs;
    iovec_iter_t iter;
    errno_t status = read_msghdr(hdr, &msg, &iovecs, &iter);
    if (status)
        return result(status, 0);

    const bool named = msg.msg_name && msg.msg_namelen;
    struct __sockaddr_storage name;
    void* control = nullptr;
    struct vfs_file* file = nullptr;
    struct socket* sock;
    size_t sent = 0;

    if (named && (status = copy_sockaddr(msg.msg_name, msg.msg_namelen, &name)))
        goto out;
    if (msg.msg_controllen) {
        // Copy the control data in from user memory.
        if (msg.msg_controllen > SOCKET_CONTROL_MAX)
            status = ENOBUFS;
        else if (!(control = mem_alloc(msg.msg_controllen, 0)))
            status = ENOMEM;
        else if (!usercopy_read(control, msg.msg_control, msg.msg_controllen))
            status = EFAULT;
        if (status)
            goto out;
    }
    if ((status = get_socket(fd, &file, &sock)))
        goto out;

    struct socket_msg kmsg = {
        .iter = &iter,
        .name = named ? &name : nullptr,
        .name_len = named ? msg.msg_namelen : 0,
        .control = control,
        .control_len = control ? msg.msg_controllen : 0,
        .flags = flags | (vfs_file_nonblocking(file) ? MSG_DONTWAIT : 0),
    };
    status = sock->ops->sendmsg ? sock->ops->sendmsg(sock, &kmsg, &sent) : EOPNOTSUPP;

out:
    if (file)
        vfs_file_unref(file);
    mem_free(control, msg.msg_controllen);
    mem_free(iovecs, msg_iov_size(&msg));
    return result(status, sent);
}

struct syscall_ret sys_recvmsg(int fd, __user struct __msghdr* hdr, uint32_t flags) {
    struct __msghdr msg;
    iovec_t* iovecs;
    iovec_iter_t iter;
    errno_t status = read_msghdr(hdr, &msg, &iovecs, &iter);
    if (status)
        return result(status, 0);

    const size_t control_cap = msg.msg_control ? MIN((size_t)msg.msg_controllen, SOCKET_CONTROL_MAX) : 0;
    struct __sockaddr_storage name = {};
    void* control = nullptr;
    struct vfs_file* file = nullptr;
    struct socket* sock;
    size_t received = 0;

    if (control_cap && !(control = mem_alloc(control_cap, 0))) {
        status = ENOMEM;
        goto out;
    }
    if ((status = get_socket(fd, &file, &sock)))
        goto out;

    struct socket_msg kmsg = {
        .iter = &iter,
        .name = msg.msg_name ? &name : nullptr,
        .control = control,
        .control_len = control_cap,
        .flags = flags | (vfs_file_nonblocking(file) ? MSG_DONTWAIT : 0),
    };
    status = sock->ops->recvmsg ? sock->ops->recvmsg(sock, &kmsg, &received) : EOPNOTSUPP;
    if (status)
        goto out;

    // Copy the name and control data out, then update the lengths and flags in the user header.
    const __socklen_t name_len = msg.msg_name ? kmsg.name_len : 0;
    const __socklen_t control_len = kmsg.control_len;
    const int out_flags = kmsg.out_flags;
    const size_t n = MIN(msg.msg_namelen, name_len);
    if ((n && !usercopy_write(msg.msg_name, &name, n)) ||
        (control_len && !usercopy_write(msg.msg_control, control, control_len)) ||
        !usercopy_write(&hdr->msg_namelen, &name_len, sizeof(name_len)) ||
        !usercopy_write(&hdr->msg_controllen, &control_len, sizeof(control_len)) ||
        !usercopy_write(&hdr->msg_flags, &out_flags, sizeof(out_flags))) {
        status = EFAULT;
        if (control_len)
            close_rights(control, control_len);
    }

out:
    if (file)
        vfs_file_unref(file);
    mem_free(control, control_cap);
    mem_free(iovecs, msg_iov_size(&msg));
    return result(status, received);
}

struct syscall_ret sys_shutdown(int fd, int how) {
    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    status = sock->ops->shutdown ? sock->ops->shutdown(sock, how) : EOPNOTSUPP;
    vfs_file_unref(file);
    return result(status, 0);
}

struct syscall_ret sys_getsockopt(int fd, int level, int name, __user void* buf, __user __socklen_t* len) {
    __socklen_t ulen;
    if (!usercopy_read(&ulen, len, sizeof(ulen)))
        return result(EFAULT, 0);

    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    uint8_t kbuf[SOCKET_OPTION_MAX] = {};
    size_t klen = MIN((size_t)ulen, SOCKET_OPTION_MAX);
    status = socket_getsockopt(sock, level, name, kbuf, &klen);
    vfs_file_unref(file);
    if (status)
        return result(status, 0);

    const __socklen_t actual = klen;
    if ((klen && !usercopy_write(buf, kbuf, klen)) || !usercopy_write(len, &actual, sizeof(actual)))
        return result(EFAULT, 0);
    return result(0, 0);
}

struct syscall_ret sys_setsockopt(int fd, int level, int name, const __user void* buf, size_t len) {
    if (len > SOCKET_OPTION_MAX)
        return result(EINVAL, 0);

    uint8_t kbuf[SOCKET_OPTION_MAX];
    if (len && !usercopy_read(kbuf, buf, len))
        return result(EFAULT, 0);

    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    status = socket_setsockopt(sock, level, name, kbuf, len);
    vfs_file_unref(file);
    return result(status, 0);
}

static struct syscall_ret do_getname(int fd, __user void* addr, __user __socklen_t* len, bool peer) {
    struct vfs_file* file;
    struct socket* sock;
    errno_t status = get_socket(fd, &file, &sock);
    if (status)
        return result(status, 0);

    status = write_name(sock, peer, addr, len);
    vfs_file_unref(file);
    return result(status, 0);
}

struct syscall_ret sys_getsockname(int fd, __user void* addr, __user __socklen_t* len) {
    return do_getname(fd, addr, len, false);
}

struct syscall_ret sys_getpeername(int fd, __user void* addr, __user __socklen_t* len) {
    return do_getname(fd, addr, len, true);
}

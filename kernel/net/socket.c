// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/mutex.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/time.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <net/socket.h>
#include <stdckdint.h>
#include <string.h>
#include <zinnia/time.h>

static const struct vfs_file_ops socket_file_ops;

errno_t socket_alloc(
    enum address_family family,
    enum sock_type type,
    int protocol,
    const struct socket_ops* ops,
    void* priv,
    struct socket** out
) {
    struct socket* sock = mem_alloc(sizeof(*sock), 0);
    if (!sock)
        return ENOMEM;

    refcount_init(&sock->rc);
    sock->ops = ops;
    sock->family = family;
    sock->type = type;
    sock->protocol = protocol;
    sock->priv = priv;
    atomic_init(&sock->opts.flags, 0);
    atomic_init(&sock->opts.rcvbuf, SOCKET_BUF_DEFAULT);
    atomic_init(&sock->opts.sndbuf, SOCKET_BUF_DEFAULT);
    atomic_init(&sock->opts.rcvtimeo_ns, 0);
    atomic_init(&sock->opts.sndtimeo_ns, 0);

    *out = sock;
    return 0;
}

void socket_ref(struct socket* sock) {
    refcount_inc(&sock->rc);
}

void socket_unref(struct socket* sock) {
    if (!refcount_dec(&sock->rc))
        return;

    if (sock->ops->destroy)
        sock->ops->destroy(sock);
    mem_free(sock, sizeof(*sock));
}

void socket_release(struct socket* sock) {
    if (sock->ops->close)
        sock->ops->close(sock);
    socket_unref(sock);
}

static mutex_t families_lock;
static tailq_head(struct socket_family) families = {nullptr, &families.tqh_first};

static struct socket_family* find_family(enum address_family family) {
    struct socket_family* f = tailq_first(&families);
    while (f != nullptr && f->family != family)
        f = tailq_next(f, link);
    return f;
}

errno_t socket_register_family(struct socket_family* family) {
    mutex_lock(&families_lock);
    if (find_family(family->family)) {
        mutex_unlock(&families_lock);
        return EEXIST;
    }
    tailq_insert_tail(&families, family, link);
    mutex_unlock(&families_lock);

    log("net: Registered new socket family \"%s\"\n", family->name);
    return 0;
}

errno_t socket_create(enum address_family family, enum sock_type type, int protocol, struct socket** out) {
    mutex_lock(&families_lock);
    struct socket_family* f = find_family(family);
    mutex_unlock(&families_lock);
    if (!f)
        return EAFNOSUPPORT;
    return f->create(type, protocol, out);
}

errno_t socket_create_pair(
    enum address_family family,
    enum sock_type type,
    int protocol,
    struct socket** out_a,
    struct socket** out_b
) {
    mutex_lock(&families_lock);
    struct socket_family* f = find_family(family);
    mutex_unlock(&families_lock);
    if (!f)
        return EAFNOSUPPORT;
    if (!f->create_pair)
        return EOPNOTSUPP;
    return f->create_pair(type, protocol, out_a, out_b);
}

errno_t socket_open_file(
    struct socket* sock,
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out
) {
    struct vfs_node* node;
    errno_t status = vfs_create_anon_inode(&socket_file_ops, VNODE_SOCKET, 0777, identity, &node);
    if (status)
        return status;

    struct vfs_file* file;
    status = vfs_file_new(node, nullptr, O_RDWR | (flags & O_NONBLOCK), &file);
    vfs_node_unref(node);
    if (status)
        return status;

    socket_ref(sock);
    file->priv = sock;
    vfs_stream_open(file);
    *out = file;
    return 0;
}

struct socket* socket_from_file(struct vfs_file* file) {
    return file->ops == &socket_file_ops ? file->priv : nullptr;
}

size_t socket_rcvbuf(struct socket* sock) {
    return atomic_load(&sock->opts.rcvbuf, ATOMIC_RELAXED);
}

const uint64_t* socket_deadline(struct socket* sock, bool send, uint64_t* storage) {
    const uint64_t ns = atomic_load(send ? &sock->opts.sndtimeo_ns : &sock->opts.rcvtimeo_ns, ATOMIC_RELAXED);
    if (!ns)
        return nullptr;
    if (ckd_add(storage, clock_get_elapsed_ns(), ns))
        *storage = UINT64_MAX;
    return storage;
}

void socket_send_sigpipe(enum msg_flags flags) {
    if (flags & MSG_NOSIGNAL)
        return;
    const struct sig_info info = signal_info_kernel();
    signal_send_task(percpu_get()->sched.current, SIGPIPE, &info);
}

static bool is_flag_option(int name) {
    switch (name) {
    case __SO_REUSEADDR:
    case __SO_REUSEPORT:
    case __SO_KEEPALIVE:
    case __SO_BROADCAST:
    case __SO_DONTROUTE:
    case __SO_OOBINLINE:
    case __SO_DEBUG:
        return true;
    default:
        return false;
    }
}

static void put_option(void* buf, size_t* len, const void* value, size_t size) {
    *len = MIN(*len, size);
    memcpy(buf, value, *len);
}

static void put_int(void* buf, size_t* len, int value) {
    put_option(buf, len, &value, sizeof(value));
}

static void put_timeout(void* buf, size_t* len, uint64_t ns) {
    const timeval_t tv = time_tv_from_nano(ns);
    put_option(buf, len, &tv, sizeof(tv));
}

errno_t socket_getsockopt(struct socket* sock, int level, int name, void* buf, size_t* len) {
    if (sock->ops->getsockopt) {
        errno_t status = sock->ops->getsockopt(sock, level, name, buf, len);
        if (status != ENOPROTOOPT)
            return status;
    }
    if (level != __SOL_SOCKET)
        return ENOPROTOOPT;

    if (is_flag_option(name)) {
        put_int(buf, len, !!(atomic_load(&sock->opts.flags, ATOMIC_RELAXED) & (1U << name)));
        return 0;
    }

    switch (name) {
    case __SO_TYPE:
        put_int(buf, len, sock->type);
        return 0;
    case __SO_DOMAIN:
        put_int(buf, len, sock->family);
        return 0;
    case __SO_PROTOCOL:
        put_int(buf, len, sock->protocol);
        return 0;
    case __SO_ERROR:
        put_int(buf, len, 0);
        return 0;
    case __SO_RCVBUF:
        put_int(buf, len, atomic_load(&sock->opts.rcvbuf, ATOMIC_RELAXED));
        return 0;
    case __SO_SNDBUF:
        put_int(buf, len, atomic_load(&sock->opts.sndbuf, ATOMIC_RELAXED));
        return 0;
    case __SO_RCVTIMEO:
        put_timeout(buf, len, atomic_load(&sock->opts.rcvtimeo_ns, ATOMIC_RELAXED));
        return 0;
    case __SO_SNDTIMEO:
        put_timeout(buf, len, atomic_load(&sock->opts.sndtimeo_ns, ATOMIC_RELAXED));
        return 0;
    default:
        return ENOPROTOOPT;
    }
}

errno_t socket_setsockopt(struct socket* sock, int level, int name, const void* buf, size_t len) {
    if (sock->ops->setsockopt) {
        errno_t status = sock->ops->setsockopt(sock, level, name, buf, len);
        if (status != ENOPROTOOPT)
            return status;
    }
    if (level != __SOL_SOCKET)
        return ENOPROTOOPT;

    if (name == __SO_RCVTIMEO || name == __SO_SNDTIMEO) {
        // Convert the timeval option value to nanoseconds.
        timeval_t tv;
        if (len < sizeof(tv))
            return EINVAL;
        memcpy(&tv, buf, sizeof(tv));

        uint64_t ns;
        if (time_tv_to_nano(&tv, &ns))
            return EDOM;
        atomic_store(name == __SO_RCVTIMEO ? &sock->opts.rcvtimeo_ns : &sock->opts.sndtimeo_ns, ns, ATOMIC_RELAXED);
        return 0;
    }

    int value;
    if (len < sizeof(value))
        return EINVAL;
    memcpy(&value, buf, sizeof(value));

    if (is_flag_option(name)) {
        if (value)
            atomic_fetch_or(&sock->opts.flags, 1U << name, ATOMIC_RELAXED);
        else
            atomic_fetch_and(&sock->opts.flags, ~(1U << name), ATOMIC_RELAXED);
        return 0;
    }

    switch (name) {
    case __SO_RCVBUF:
    case __SO_RCVBUFFORCE:
    case __SO_SNDBUF:
    case __SO_SNDBUFFORCE: {
        const size_t size = MIN(MAX((size_t)MAX(value, 0), SOCKET_BUF_MIN), SOCKET_BUF_MAX);
        const bool rcv = name == __SO_RCVBUF || name == __SO_RCVBUFFORCE;
        atomic_store(rcv ? &sock->opts.rcvbuf : &sock->opts.sndbuf, size, ATOMIC_RELAXED);
        return 0;
    }
    default:
        return ENOPROTOOPT;
    }
}

static enum msg_flags file_msg_flags(struct vfs_file* file) {
    return vfs_file_nonblocking(file) ? MSG_DONTWAIT : 0;
}

static errno_t socket_file_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct socket* sock = file->priv;
    struct socket_msg msg = {.iter = buffer, .flags = file_msg_flags(file)};
    size_t received = 0;
    errno_t status = sock->ops->recvmsg ? sock->ops->recvmsg(sock, &msg, &received) : EOPNOTSUPP;
    *out_read = received;
    return status;
}

static errno_t socket_file_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct socket* sock = file->priv;
    struct socket_msg msg = {.iter = buffer, .flags = file_msg_flags(file)};
    size_t sent = 0;
    errno_t status = sock->ops->sendmsg ? sock->ops->sendmsg(sock, &msg, &sent) : EOPNOTSUPP;
    *out_written = sent;
    return status;
}

static errno_t socket_file_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    struct socket* sock = file->priv;
    if (sock->ops->ioctl) {
        errno_t status = sock->ops->ioctl(sock, request, arg, out_result);
        if (status != ENOTTY)
            return status;
    }
    // TODO: netif ioctls (SIOCGIF*)
    return ENOTTY;
}

static enum poll_flags socket_file_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table) {
    struct socket* sock = file->priv;
    return sock->ops->poll ? sock->ops->poll(sock, events, table) : 0;
}

static void socket_file_release(struct vfs_file* file) {
    socket_release(file->priv);
}

static const struct vfs_file_ops socket_file_ops = {
    .read = socket_file_read,
    .write = socket_file_write,
    .ioctl = socket_file_ioctl,
    .poll = socket_file_poll,
    .release = socket_file_release,
};

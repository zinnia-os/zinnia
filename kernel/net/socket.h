// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/poll.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/iovec.h>
#include <lib/refcount.h>
#include <lib/tailq.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/socket.h>

enum address_family : uint16_t {
    AF_UNSPEC = __AF_UNSPEC,
    AF_INET = __AF_INET,
    AF_INET6 = __AF_INET6,
    AF_UNIX = __AF_UNIX,
    AF_PACKET = __AF_PACKET,
};

enum sock_type : uint32_t {
    SOCK_DGRAM = __SOCK_DGRAM,
    SOCK_RAW = __SOCK_RAW,
    SOCK_STREAM = __SOCK_STREAM,
    SOCK_SEQPACKET = __SOCK_SEQPACKET,
    SOCK_NONBLOCK = __SOCK_NONBLOCK,
    SOCK_CLOEXEC = __SOCK_CLOEXEC,
    SOCK_CLOFORK = __SOCK_CLOFORK,
};

enum msg_flags : uint32_t {
    MSG_CTRUNC = __MSG_CTRUNC,
    MSG_DONTROUTE = __MSG_DONTROUTE,
    MSG_EOR = __MSG_EOR,
    MSG_OOB = __MSG_OOB,
    MSG_NOSIGNAL = __MSG_NOSIGNAL,
    MSG_PEEK = __MSG_PEEK,
    MSG_TRUNC = __MSG_TRUNC,
    MSG_WAITALL = __MSG_WAITALL,
    MSG_FIN = __MSG_FIN,
    MSG_CONFIRM = __MSG_CONFIRM,
    MSG_DONTWAIT = __MSG_DONTWAIT,
    MSG_CMSG_CLOEXEC = __MSG_CMSG_CLOEXEC,
    MSG_MORE = __MSG_MORE,
};

enum shutdown_flags : uint32_t {
    SHUT_RD = __SHUT_RD,
    SHUT_WR = __SHUT_WR,
    SHUT_RDWR = __SHUT_RDWR,
};

constexpr size_t SOCKET_BUF_DEFAULT = 0x10000;
constexpr size_t SOCKET_BUF_MIN = 0x1000;
constexpr size_t SOCKET_BUF_MAX = 0x100000;

struct socket;

struct socket_msg {
    iovec_iter_t* iter;
    struct __sockaddr_storage* name;
    size_t name_len;
    void* control;
    size_t control_len;
    enum msg_flags flags;
    enum msg_flags out_flags;
};

struct socket_ops {
    errno_t (*bind)(struct socket* sock, const struct __sockaddr_storage* addr, size_t len);
    errno_t (*listen)(struct socket* sock, int backlog);
    errno_t (*accept)(struct socket* sock, bool nonblock, struct socket** out);
    errno_t (*connect)(struct socket* sock, const struct __sockaddr_storage* addr, size_t len, bool nonblock);
    errno_t (*disconnect)(struct socket* sock);
    errno_t (*sendmsg)(struct socket* sock, struct socket_msg* msg, size_t* out_sent);
    errno_t (*recvmsg)(struct socket* sock, struct socket_msg* msg, size_t* out_received);
    errno_t (*shutdown)(struct socket* sock, enum shutdown_flags how);
    errno_t (*getsockname)(struct socket* sock, struct __sockaddr_storage* out, size_t* out_len);
    errno_t (*getpeername)(struct socket* sock, struct __sockaddr_storage* out, size_t* out_len);
    errno_t (*getsockopt)(struct socket* sock, int level, int name, void* buf, size_t* len);
    errno_t (*setsockopt)(struct socket* sock, int level, int name, const void* buf, size_t len);
    errno_t (*ioctl)(struct socket* sock, uintptr_t request, uintptr_t arg, uintptr_t* out_result);
    enum poll_flags (*poll)(struct socket* sock, enum poll_flags events, struct poll_table* table);
    void (*close)(struct socket* sock);
    void (*destroy)(struct socket* sock);
};

struct socket_opts {
    _Atomic(uint32_t) flags;
    _Atomic(size_t) rcvbuf;
    _Atomic(size_t) sndbuf;
    _Atomic(uint64_t) rcvtimeo_ns;
    _Atomic(uint64_t) sndtimeo_ns;
};

struct socket {
    struct refcount rc;
    const struct socket_ops* ops;
    enum address_family family;
    enum sock_type type;
    int protocol;
    struct socket_opts opts;
    void* priv;
};

struct socket_family {
    const char* name;
    enum address_family family;
    // Create a new socket of this family. Not optional to implement.
    errno_t (*create)(enum sock_type type, int protocol, struct socket** out);
    // Create a new pair of sockets of this family. Optional.
    errno_t (*create_pair)(enum sock_type type, int protocol, struct socket** out_a, struct socket** out_b);
    tailq_link(struct socket_family) link;
};

errno_t socket_register_family(struct socket_family* family);

// Creates a new socket object.
errno_t socket_create(enum address_family family, enum sock_type type, int protocol, struct socket** out);
// Creates a pair of sockets pointing to each other.
errno_t socket_create_pair(
    enum address_family family,
    enum sock_type type,
    int protocol,
    struct socket** out_a,
    struct socket** out_b
);

errno_t socket_open_file(
    struct socket* sock,
    enum open_flags flags,
    const struct ident* identity,
    struct vfs_file** out
);
struct socket* socket_from_file(struct vfs_file* file);

// Allocates a new socket object.
errno_t socket_alloc(
    enum address_family family,
    enum sock_type type,
    int protocol,
    const struct socket_ops* ops,
    void* priv,
    struct socket** out
);
void socket_ref(struct socket* sock);
void socket_unref(struct socket* sock);
void socket_release(struct socket* sock);

errno_t socket_getsockopt(struct socket* sock, int level, int name, void* buf, size_t* len);
errno_t socket_setsockopt(struct socket* sock, int level, int name, const void* buf, size_t len);

size_t socket_rcvbuf(struct socket* sock);
const uint64_t* socket_deadline(struct socket* sock, bool send, uint64_t* storage);
void socket_send_sigpipe(enum msg_flags flags);

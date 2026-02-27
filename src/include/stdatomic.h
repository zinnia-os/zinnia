#ifndef ZINNIA_STDATOMIC_H
#define ZINNIA_STDATOMIC_H

typedef enum memory_order {
    memory_order_relaxed = __ATOMIC_RELAXED,
    memory_order_consume = __ATOMIC_CONSUME,
    memory_order_acquire = __ATOMIC_ACQUIRE,
    memory_order_release = __ATOMIC_RELEASE,
    memory_order_acq_rel = __ATOMIC_ACQ_REL,
    memory_order_seq_cst = __ATOMIC_SEQ_CST
} memory_order;

#define atomic_store_explicit(ptr, des, ord) __atomic_store_n(ptr, des, ord)
#define atomic_store(ptr, des)               atomic_store_explicit(ptr, des, __ATOMIC_SEQ_CST)

#define atomic_load_explicit(ptr, ord) __atomic_load_n(ptr, ord)
#define atomic_load(ptr)               atomic_load_explicit(ptr, __ATOMIC_SEQ_CST)

#define atomic_exchange_explicit(ptr, des, ord) __atomic_exchange_n(ptr, des, ord)
#define atomic_exchange(ptr, des)               atomic_exchange_explicit(ptr, des, __ATOMIC_SEQ_CST)

#define atomic_fetch_add_explicit(ptr, op, ord) __atomic_fetch_add(ptr, op, ord)
#define atomic_fetch_add(ptr, op)               atomic_fetch_add_explicit(ptr, op, __ATOMIC_SEQ_CST)

#define atomic_fetch_sub_explicit(ptr, op, ord) __atomic_fetch_sub(ptr, op, ord)
#define atomic_fetch_sub(ptr, op)               atomic_fetch_sub_explicit(ptr, op, __ATOMIC_SEQ_CST)

#define atomic_fetch_or_explicit(ptr, op, ord) __atomic_fetch_or(ptr, op, ord)
#define atomic_fetch_or(ptr, op)               atomic_fetch_or_explicit(ptr, op, __ATOMIC_SEQ_CST)

#define atomic_fetch_xor_explicit(ptr, op, ord) __atomic_fetch_xor(ptr, op, ord)
#define atomic_fetch_xor(ptr, op)               atomic_fetch_xor_explicit(ptr, op, __ATOMIC_SEQ_CST)

#define atomic_fetch_and_explicit(ptr, op, ord) __atomic_fetch_and(ptr, op, ord)
#define atomic_fetch_and(ptr, op)               atomic_fetch_and_explicit(ptr, op, __ATOMIC_SEQ_CST)

#endif

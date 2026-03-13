#pragma once

#define TAILQ_HEAD(type) \
    struct { \
        type* tqh_first; \
        type** tqh_last; \
    }

#define TAILQ_LINK(type) \
    struct { \
        type* tqe_next; \
        type** tqe_prev; \
    }

#define TAILQ_INIT(head) \
    do { \
        (head)->tqh_first = nullptr; \
        (head)->tqh_last = &(head)->tqh_first; \
    } while (0)

#define TAILQ_FIRST(head) ((head)->tqh_first)

#define TAILQ_NEXT(elm, field) ((elm)->field.tqe_next)

#define TAILQ_EMPTY(head) (TAILQ_FIRST(head) == nullptr)

#define TAILQ_INSERT_TAIL(head, elm, field) \
    do { \
        (elm)->field.tqe_next = nullptr; \
        (elm)->field.tqe_prev = (head)->tqh_last; \
        *(head)->tqh_last = (elm); \
        (head)->tqh_last = &(elm)->field.tqe_next; \
    } while (0)

#define TAILQ_INSERT_HEAD(head, elm, field) \
    do { \
        if (((elm)->field.tqe_next = (head)->tqh_first) != nullptr) \
            (head)->tqh_first->field.tqe_prev = &(elm)->field.tqe_next; \
        else \
            (head)->tqh_last = &(elm)->field.tqe_next; \
        (head)->tqh_first = (elm); \
        (elm)->field.tqe_prev = &(head)->tqh_first; \
    } while (0)

#define TAILQ_REMOVE(head, elm, field) \
    do { \
        if (((elm)->field.tqe_next) != nullptr) \
            (elm)->field.tqe_next->field.tqe_prev = (elm)->field.tqe_prev; \
        else \
            (head)->tqh_last = (elm)->field.tqe_prev; \
        *(elm)->field.tqe_prev = (elm)->field.tqe_next; \
    } while (0)

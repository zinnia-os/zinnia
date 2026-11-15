// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#define tailq_head(type) \
    struct { \
        type* tqh_first; \
        type** tqh_last; \
    }

#define tailq_link(type) \
    struct { \
        type* tql_next; \
        type** tql_prev; \
    }

#define tailq_init(head) \
    do { \
        (head)->tqh_first = nullptr; \
        (head)->tqh_last = &(head)->tqh_first; \
    } while (0)

#define tailq_first(head) ((head)->tqh_first)

#define tailq_next(elm, field) ((elm)->field.tql_next)

#define tailq_empty(head) (tailq_first(head) == nullptr)

#define tailq_insert_tail(head, elm, field) \
    do { \
        (elm)->field.tql_next = nullptr; \
        (elm)->field.tql_prev = (head)->tqh_last; \
        *(head)->tqh_last = (elm); \
        (head)->tqh_last = &(elm)->field.tql_next; \
    } while (0)

#define tailq_insert_head(head, elm, field) \
    do { \
        if (((elm)->field.tql_next = (head)->tqh_first) != nullptr) \
            (head)->tqh_first->field.tql_prev = &(elm)->field.tql_next; \
        else \
            (head)->tqh_last = &(elm)->field.tql_next; \
        (head)->tqh_first = (elm); \
        (elm)->field.tql_prev = &(head)->tqh_first; \
    } while (0)

#define tailq_insert_before(listelm, elm, field) \
    do { \
        (elm)->field.tql_prev = (listelm)->field.tql_prev; \
        (elm)->field.tql_next = (listelm); \
        *(listelm)->field.tql_prev = (elm); \
        (listelm)->field.tql_prev = &(elm)->field.tql_next; \
    } while (0)

#define tailq_remove(head, elm, field) \
    do { \
        if (((elm)->field.tql_next) != nullptr) \
            (elm)->field.tql_next->field.tql_prev = (elm)->field.tql_prev; \
        else \
            (head)->tqh_last = (elm)->field.tql_prev; \
        *(elm)->field.tql_prev = (elm)->field.tql_next; \
    } while (0)

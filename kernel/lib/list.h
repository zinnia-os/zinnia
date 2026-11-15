// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#define list_head(type) \
    struct { \
        type* _sl_head; \
    }

#define list_link(type) \
    struct { \
        type* _sl_link; \
    }

#define list_is_empty(head)   ((head)->_sl_head == nullptr)
#define list_first(head)      ((head)->_sl_head)
#define list_next(elm, field) ((elm)->field._sl_link)

#define list_insert_end(slistelm, elm, field) \
    do { \
        list_next((elm), field) = list_next((slistelm), field); \
        list_next((slistelm), field) = (elm); \
    } while (0)

#define list_insert_head(head, elm, field) \
    do { \
        list_next((elm), field) = list_first((head)); \
        list_first((head)) = (elm); \
    } while (0)

#define list_foreach(var, head, field) for ((var) = list_first((head)); (var); (var) = list_next((var), field))

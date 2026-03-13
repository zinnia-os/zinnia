#pragma once

#define SLIST_HEAD(type) \
    struct { \
        type sl_head; \
    }

#define SLIST_LINK(type) \
    struct { \
        type sl_link; \
    }

#define SLIST_EMPTY(head)      ((head)->sl_head == NULL)
#define SLIST_FIRST(head)      ((head)->sl_head)
#define SLIST_NEXT(elm, field) ((elm)->field.sl_link)

#define SLIST_INSERT_END(slistelm, elm, field) \
    do { \
        SLIST_NEXT((elm), field) = SLIST_NEXT((slistelm), field); \
        SLIST_NEXT((slistelm), field) = (elm); \
    } while (0)

#define SLIST_INSERT_HEAD(head, elm, field) \
    do { \
        SLIST_NEXT((elm), field) = SLIST_FIRST((head)); \
        SLIST_FIRST((head)) = (elm); \
    } while (0)

#define SLIST_FOREACH(var, head, field) for ((var) = SLIST_FIRST((head)); (var); (var) = SLIST_NEXT((var), field))

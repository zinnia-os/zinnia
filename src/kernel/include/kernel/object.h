#pragma once

#include <stddef.h>

struct object {
    void (*drop)(struct object* obj);
};

void object_get(struct object* obj);
void object_put(struct object* obj);

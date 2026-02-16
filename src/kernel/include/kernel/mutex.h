#pragma once

struct task;

struct mutex {
    struct task* owner;
    bool flag;
};

void mutex_lock(struct mutex* mutex);
void mutex_unlock(struct mutex* mutex);

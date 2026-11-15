// SPDX-License-Identifier: BSD-3-Clause

#pragma once

struct task;

// TODO
static inline bool signal_pending(struct task*) {
    return false;
}

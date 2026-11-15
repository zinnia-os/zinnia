// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/sched.h>

errno_t task_create_current(const char* name, struct process* proc, struct task** out);

bool sched_update_priority_locked(struct task* task);

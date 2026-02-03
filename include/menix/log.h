#ifndef MENIX_LOG_H
#define MENIX_LOG_H

#include <menix/status.h>

// Panics and returns an error status to the parent process.
void menix_panic(menix_status_t status);

void menix_log(const char* message);

#endif

#ifndef ZINNIA_SYSTEM_H
#define ZINNIA_SYSTEM_H

#include <zinnia/status.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

void zn_log(const char* message, size_t len);

// Returns the page size of the system.
size_t zn_page_size();

zn_status_t zn_powerctl();

#ifdef __cplusplus
}
#endif

#endif

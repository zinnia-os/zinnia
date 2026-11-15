// SPDX-License-Identifier: BSD-3-Clause

#include <lib/guid.h>
#include <lib/print.h>
#include <string.h>

bool guid_is_zero(const struct guid* guid) {
    static const struct guid zero = {};
    return guid_equal(guid, &zero);
}

bool guid_equal(const struct guid* a, const struct guid* b) {
    return memcmp(a, b, sizeof(struct guid)) == 0;
}

void guid_format(const struct guid* guid, char out[static GUID_STR_LEN]) {
    ksnprintf(
        out,
        GUID_STR_LEN,
        "%08x-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x",
        guid->time_low,
        guid->time_mid,
        guid->time_hi_version,
        guid->clock_seq[0],
        guid->clock_seq[1],
        guid->node[0],
        guid->node[1],
        guid->node[2],
        guid->node[3],
        guid->node[4],
        guid->node[5]
    );
}

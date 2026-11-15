// SPDX-License-Identifier: BSD-3-Clause

#include <lib/ktest.h>

KTEST(example, arithmetic) {
    KTEST_EXPECT_EQ(1 + 1, 2);
    KTEST_EXPECT_LT(1, 2);
}

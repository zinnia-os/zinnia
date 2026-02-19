#pragma once

#include <common/compiler.h>

#define VDSO_FUNC(ret, name, ...) \
    [[__weak, __alias("__vdso_" #name)]] \
    ret name(__VA_ARGS__); \
    [[__used]] \
    ret __vdso_##name(__VA_ARGS__)

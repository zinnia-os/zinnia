#pragma once

typedef unsigned long size_t;
typedef signed long ssize_t;

#define offsetof(type, member) __builtin_offsetof(type, member)

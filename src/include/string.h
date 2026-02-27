#ifndef ZINNIA_STRING_H
#define ZINNIA_STRING_H

#include <stddef.h>

size_t strlen(const char* str);
size_t strnlen(const char* str, size_t len);
int memcmp(const void* s1, const void* s2, size_t size);
void* memcpy(void* restrict dest, const void* restrict src, size_t n);
void* memmove(void* dstptr, const void* srcptr, size_t size);
void* memset(void* dest, int value, size_t n);
char* strncpy(char* restrict dst, const char* restrict src, size_t len);
int strcmp(const char* str1, const char* str2);
int strncmp(const char* str1, const char* str2, size_t len);
char* strchr(const char* c, int s);

#endif

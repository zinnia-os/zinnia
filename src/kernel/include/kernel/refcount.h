
// SPDX-License-Identifier: MIT

#pragma once

// Generic reference counting struct.
struct rc {
    // Number of references to the data.
    int refcount;
    // Pointer to the data.
    void* data;
    // Cleanup function for the data.
    void (*cleanup)(void*);
};

// Create a new refcount pointer, return NULL if out of memory.
// If the creation fails, it is up to the user to clean up `data`.
struct rc* rc_new(void* data, void (*cleanup)(void*));
// Create a new refcount pointer, abort if out of memory.
struct rc* rc_new_strong(void* data, void (*cleanup)(void*));
// Take a new share from a refcount pointer.
struct rc* rc_share(struct rc* rc);
// Delete a share from a refcount pointer.
void rc_delete(struct rc* rc);

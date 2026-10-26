#ifndef VEC_H
#define VEC_H

#include <stddef.h>

/* Growable array of int. All functions return 0 on success and -1 on error. */
typedef struct vec {
    int *data;
    size_t len;
    size_t cap;
} vec_t;

/* Allocate an empty vector. Returns NULL on allocation failure. */
vec_t *vec_new(void);

/* Release the vector and its storage. Safe to call with NULL. */
void vec_free(vec_t *v);

/* Append value, growing the storage if needed. */
int vec_push(vec_t *v, int value);

/* Copy the element at index into *out. Fails if index >= len. */
int vec_get(const vec_t *v, size_t index, int *out);

/* Number of stored elements. */
size_t vec_len(const vec_t *v);

#endif /* VEC_H */

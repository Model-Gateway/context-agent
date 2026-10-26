#include "vec.h"

#include <stdlib.h>

#define VEC_INITIAL_CAP 4

vec_t *vec_new(void)
{
    vec_t *v = malloc(sizeof *v);
    if (v == NULL) {
        return NULL;
    }
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
    return v;
}

void vec_free(vec_t *v)
{
    if (v == NULL) {
        return;
    }
    free(v->data);
    free(v);
}

static int vec_grow(vec_t *v)
{
    size_t new_cap = v->cap == 0 ? VEC_INITIAL_CAP : v->cap * 2;
    int *next = realloc(v->data, new_cap * sizeof *next);
    if (next == NULL) {
        return -1;
    }
    v->data = next;
    v->cap = new_cap;
    return 0;
}

int vec_push(vec_t *v, int value)
{
    if (v == NULL) {
        return -1;
    }
    if (v->len == v->cap && vec_grow(v) != 0) {
        return -1;
    }
    v->data[v->len++] = value;
    return 0;
}

int vec_get(const vec_t *v, size_t index, int *out)
{
    if (v == NULL || out == NULL || index >= v->len) {
        return -1; /* index out of range */
    }
    *out = v->data[index];
    return 0;
}

size_t vec_len(const vec_t *v)
{
    return v == NULL ? 0 : v->len;
}

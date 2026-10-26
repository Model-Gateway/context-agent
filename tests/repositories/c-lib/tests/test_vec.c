#include "vec.h"

#include <assert.h>
#include <stdio.h>

static void test_new_is_empty(void)
{
    vec_t *v = vec_new();
    assert(v != NULL);
    assert(vec_len(v) == 0);
    vec_free(v);
}

static void test_push_and_get(void)
{
    vec_t *v = vec_new();
    int out = 0;
    assert(vec_push(v, 10) == 0);
    assert(vec_push(v, 20) == 0);
    assert(vec_len(v) == 2);
    assert(vec_get(v, 0, &out) == 0 && out == 10);
    assert(vec_get(v, 1, &out) == 0 && out == 20);
    vec_free(v);
}

static void test_get_out_of_range_fails(void)
{
    vec_t *v = vec_new();
    int out = 42;
    assert(vec_get(v, 0, &out) == -1);
    assert(out == 42);
    vec_free(v);
}

static void test_grows_past_initial_capacity(void)
{
    vec_t *v = vec_new();
    int i;
    int out = 0;
    for (i = 0; i < 100; i++) {
        assert(vec_push(v, i * i) == 0);
    }
    assert(vec_len(v) == 100);
    assert(vec_get(v, 99, &out) == 0 && out == 99 * 99);
    vec_free(v);
}

int main(void)
{
    test_new_is_empty();
    test_push_and_get();
    test_get_out_of_range_fails();
    test_grows_past_initial_capacity();
    puts("test_vec: all tests passed");
    return 0;
}

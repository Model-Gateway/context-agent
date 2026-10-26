# c-lib

A growable array of `int` written in C99, used as a test fixture for Context
Agent. No dependencies beyond libc.

- `include/vec.h` – public API (`vec_new`, `vec_free`, `vec_push`, `vec_get`, `vec_len`).
- `src/vec.c`     – implementation; doubles capacity on growth.
- `tests/test_vec.c` – `assert`-based tests.

```sh
make test   # builds and runs tests/test_vec
make clean
```

Functions return `0` on success and `-1` on failure (out of memory, index out
of range); they never abort.

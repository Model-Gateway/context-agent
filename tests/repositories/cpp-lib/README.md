# cpp-lib

A small dense `Matrix` class (row-major `std::vector<double>`), used as a test
fixture for Context. C++17, no dependencies.

- `include/matrix.hpp` – class declaration.
- `src/matrix.cpp`     – `add`, `multiply`, element access.
- `tests/test_matrix.cpp` – `assert`-based tests.

```sh
make test
make clean
```

Shape mismatches throw `std::invalid_argument`; out-of-range element access
throws `std::out_of_range`.

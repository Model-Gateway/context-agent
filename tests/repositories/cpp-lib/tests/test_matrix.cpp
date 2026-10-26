#include "matrix.hpp"

#include <cassert>
#include <iostream>
#include <stdexcept>

using linalg::Matrix;

static void test_construct_and_access() {
    Matrix m(2, 3);
    assert(m.rows() == 2);
    assert(m.cols() == 3);
    m.at(1, 2) = 7.5;
    assert(m.at(1, 2) == 7.5);
}

static void test_add() {
    Matrix a{{1, 2}, {3, 4}};
    Matrix b{{10, 20}, {30, 40}};
    Matrix expected{{11, 22}, {33, 44}};
    assert(a.add(b) == expected);
}

static void test_multiply() {
    Matrix a{{1, 2, 3}, {4, 5, 6}};
    Matrix b{{7, 8}, {9, 10}, {11, 12}};
    Matrix expected{{58, 64}, {139, 154}};
    assert(a.multiply(b) == expected);
}

static void test_shape_mismatch_throws() {
    Matrix a(2, 2);
    Matrix b(3, 3);
    bool threw = false;
    try {
        a.add(b);
    } catch (const std::invalid_argument&) {
        threw = true;
    }
    assert(threw);
}

static void test_out_of_range_throws() {
    Matrix a(1, 1);
    bool threw = false;
    try {
        a.at(1, 0);
    } catch (const std::out_of_range&) {
        threw = true;
    }
    assert(threw);
}

int main() {
    test_construct_and_access();
    test_add();
    test_multiply();
    test_shape_mismatch_throws();
    test_out_of_range_throws();
    std::cout << "test_matrix: all tests passed\n";
    return 0;
}

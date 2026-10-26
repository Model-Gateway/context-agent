#ifndef MATRIX_HPP
#define MATRIX_HPP

#include <cstddef>
#include <initializer_list>
#include <vector>

namespace linalg {

/// Dense row-major matrix of doubles.
class Matrix {
public:
    Matrix(std::size_t rows, std::size_t cols);
    Matrix(std::initializer_list<std::initializer_list<double>> rows);

    std::size_t rows() const { return rows_; }
    std::size_t cols() const { return cols_; }

    double& at(std::size_t r, std::size_t c);
    double at(std::size_t r, std::size_t c) const;

    /// Element-wise sum. Throws std::invalid_argument on shape mismatch.
    Matrix add(const Matrix& other) const;

    /// Matrix product. Throws std::invalid_argument when cols() != other.rows().
    Matrix multiply(const Matrix& other) const;

    bool operator==(const Matrix& other) const;
    bool operator!=(const Matrix& other) const { return !(*this == other); }

private:
    std::size_t rows_;
    std::size_t cols_;
    std::vector<double> data_;

    std::size_t index(std::size_t r, std::size_t c) const;
};

}  // namespace linalg

#endif  // MATRIX_HPP

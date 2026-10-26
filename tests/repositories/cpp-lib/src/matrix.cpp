#include "matrix.hpp"

#include <stdexcept>

namespace linalg {

Matrix::Matrix(std::size_t rows, std::size_t cols)
    : rows_(rows), cols_(cols), data_(rows * cols, 0.0) {}

Matrix::Matrix(std::initializer_list<std::initializer_list<double>> rows)
    : rows_(rows.size()), cols_(rows.size() ? rows.begin()->size() : 0), data_() {
    data_.reserve(rows_ * cols_);
    for (const auto& row : rows) {
        if (row.size() != cols_) {
            throw std::invalid_argument("Matrix rows must all have the same length");
        }
        data_.insert(data_.end(), row.begin(), row.end());
    }
}

std::size_t Matrix::index(std::size_t r, std::size_t c) const {
    if (r >= rows_ || c >= cols_) {
        throw std::out_of_range("Matrix index out of range");
    }
    return r * cols_ + c;
}

double& Matrix::at(std::size_t r, std::size_t c) { return data_[index(r, c)]; }

double Matrix::at(std::size_t r, std::size_t c) const { return data_[index(r, c)]; }

Matrix Matrix::add(const Matrix& other) const {
    if (rows_ != other.rows_ || cols_ != other.cols_) {
        throw std::invalid_argument("Matrix::add requires matching shapes");
    }
    Matrix result(rows_, cols_);
    for (std::size_t i = 0; i < data_.size(); ++i) {
        result.data_[i] = data_[i] + other.data_[i];
    }
    return result;
}

Matrix Matrix::multiply(const Matrix& other) const {
    if (cols_ != other.rows_) {
        throw std::invalid_argument("Matrix::multiply requires cols() == other.rows()");
    }
    Matrix result(rows_, other.cols_);
    for (std::size_t r = 0; r < rows_; ++r) {
        for (std::size_t c = 0; c < other.cols_; ++c) {
            double sum = 0.0;
            for (std::size_t k = 0; k < cols_; ++k) {
                sum += at(r, k) * other.at(k, c);
            }
            result.at(r, c) = sum;
        }
    }
    return result;
}

bool Matrix::operator==(const Matrix& other) const {
    return rows_ == other.rows_ && cols_ == other.cols_ && data_ == other.data_;
}

}  // namespace linalg

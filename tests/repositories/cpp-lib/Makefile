# cpp-lib: a small dense matrix class. GNU make 3.81 compatible.
CXX      ?= c++
CXXFLAGS ?= -std=c++17 -Wall -Wextra -pedantic -O1
CPPFLAGS  = -Iinclude

OBJ      = src/matrix.o
TEST_BIN = tests/test_matrix

.PHONY: all test clean

all: $(OBJ)

src/matrix.o: src/matrix.cpp include/matrix.hpp
	$(CXX) $(CXXFLAGS) $(CPPFLAGS) -c -o $@ src/matrix.cpp

$(TEST_BIN): tests/test_matrix.cpp $(OBJ) include/matrix.hpp
	$(CXX) $(CXXFLAGS) $(CPPFLAGS) -o $@ tests/test_matrix.cpp $(OBJ)

test: $(TEST_BIN)
	./$(TEST_BIN)

clean:
	rm -f $(OBJ) $(TEST_BIN)

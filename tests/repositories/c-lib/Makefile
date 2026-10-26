# c-lib: a dynamic int array. GNU make 3.81 compatible.
CC      ?= cc
CFLAGS  ?= -std=c99 -Wall -Wextra -pedantic -O1
CPPFLAGS = -Iinclude

OBJ      = src/vec.o
TEST_BIN = tests/test_vec

.PHONY: all test clean

all: $(OBJ)

src/vec.o: src/vec.c include/vec.h
	$(CC) $(CFLAGS) $(CPPFLAGS) -c -o $@ src/vec.c

$(TEST_BIN): tests/test_vec.c $(OBJ) include/vec.h
	$(CC) $(CFLAGS) $(CPPFLAGS) -o $@ tests/test_vec.c $(OBJ)

test: $(TEST_BIN)
	./$(TEST_BIN)

clean:
	rm -f $(OBJ) $(TEST_BIN)

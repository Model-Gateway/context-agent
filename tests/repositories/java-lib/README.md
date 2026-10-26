# java-lib

A small calculator library used as a test fixture for Context.

- `Calculator` – arithmetic primitives (`add`, `subtract`, `multiply`, `divide`).
- `Parser`     – evaluates simple infix expressions such as `"2 + 3"` using `Calculator`.

Tests use JUnit 4 and run with:

```sh
mvn -q test
```

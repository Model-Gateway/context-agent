# rust-cli

A tiny command-line tool used as a test fixture for Context. The crate is
split into a library (`src/lib.rs`) that holds all logic and a thin binary
(`src/main.rs`) that only deals with process I/O.

## Commands

| Command | Example                      | Output          |
| ------- | ---------------------------- | --------------- |
| `greet` | `rust-cli greet Ada`         | `Hello, Ada!`   |
| `shout` | `rust-cli shout hi there`    | `HI THERE!`     |

## Layout

- `src/parser.rs`   – turns raw argv into a `Command` (`parse_args`).
- `src/commands.rs` – one pure function per command (`greet`, `shout`).
- `src/lib.rs`      – `run()` dispatches a parsed `Command` to its function.
- `tests/cli.rs`    – integration tests that exercise the library and the binary.

Adding a command means: add a function to `commands.rs`, a match arm in
`lib.rs::run`, a row in the table above and a test in `tests/cli.rs`.

## Tests

```sh
cargo test
```

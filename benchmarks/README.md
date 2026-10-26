# Context benchmarks

Reproducible, self-contained benchmarks for the Context coding agent. Every
task runs against one of the dependency-free fixture repositories under
`tests/repositories/`, and every number this suite produces is reported with its
sample count and a confidence interval.

## Layout

```
benchmarks/
  README.md                 this file: organisation, schema, protocol, reporting
  tasks/<id>.toml           one agent task per file (schema below)
  retrieval/queries.jsonl   labelled retrieval queries, one JSON object per line
  results/<run-id>/         run outputs (git-ignored via /benchmarks/results/)
tests/repositories/<name>/  the fixtures the tasks and queries point at
```

The suite has two parts that can be run independently:

- **Agent tasks** (`tasks/`): end-to-end runs where the agent reads a prompt,
  edits a scratch copy of a fixture, and a list of shell commands decides whether
  the result is acceptable. They also yield retrieval recall, because each task
  names the files a correct change touches.
- **Retrieval queries** (`retrieval/`): offline evaluation of the retrieval layer
  alone. Each query names the files a good retriever should rank first; no model
  calls are needed.

## Fixtures

All fixtures are plain directories (no `.git`, no lockfiles, no third-party
packages, fewer than 15 source files each). The validation commands below are the
ones the task files use; they were run on the authoring machine except where noted.

| Fixture      | Languages              | Tools needed          | Baseline validation command                                   | Pristine state                         |
| ------------ | ---------------------- | --------------------- | ------------------------------------------------------------- | -------------------------------------- |
| `ts-web-app` | TypeScript / TSX       | `node` (>= 20)        | `node --test src/__tests__/`                                  | 6 tests pass                           |
| `py-service` | Python                 | `python3`             | `python3 -m unittest discover -s tests -v`                    | 8 tests, **1 fails by design**         |
| `rust-cli`   | Rust                   | `cargo`               | `cargo test --offline`                                        | 12 tests pass                          |
| `go-service` | Go                     | `go`                  | `go test ./...`                                               | 3 tests pass                           |
| `java-lib`   | Java                   | `mvn`, JDK            | `mvn -q test`                                                 | reviewed only (no JDK here)            |
| `c-lib`      | C99                    | `make`, `cc`          | `make test`                                                   | passes                                 |
| `cpp-lib`    | C++17                  | `make`, `c++`         | `make test`                                                   | passes                                 |
| `ruby-gem`   | Ruby (>= 2.6)          | `ruby`                | `ruby -Ilib -Itest -e 'Dir["test/*_test.rb"].each { ... }'`   | 10 tests pass                          |
| `php-app`    | PHP (>= 8.1)           | `php`, `composer`     | `composer install && composer test`                           | reviewed only (no PHP here)            |
| `polyglot`   | Python + JS + Rust     | `python3`, `node`, `cargo` | `make test`; `cargo test --manifest-path tools/Cargo.toml` | 13 + 10 + 2 tests pass                 |

The one intentional red state is `py-service`: `test_pricing.PricingTests.test_ten_percent_off`
fails (`190.0 != 180.0`) because `apply_discount` subtracts the percentage as an
absolute amount. Tasks that are not about that bug avoid depending on the full
suite (see `py-rename-apply-discount`).

## Task schema

One TOML file per task, `tasks/<id>.toml`, with exactly these keys:

```toml
id = "ts-login-loading"                  # unique, kebab-case, equals the file name
repository = "tests/repositories/ts-web-app"   # relative to the workspace root
category = "localized_edit"              # localized_edit | multi_file_change | complex_feature | investigation
languages = ["typescript", "tsx"]
prompt = "Add a loading indicator to the login button using the existing Spinner component while authentication is in progress."
validation = ["node --test src/__tests__/"]   # commands that must exit 0 afterwards (run from the repository dir)
expected_files = ["src/components/LoginButton.tsx"]   # files the agent should touch (for retrieval recall)
requires_tools = ["node"]                # executables that must exist for the task to be runnable
timeout_secs = 600
[[turns]]                                # optional follow-up turns for multi-turn sequences
prompt = "Make the spinner smaller."
expected_files = ["src/components/LoginButton.tsx"]
```

Field semantics:

- `id`: `^[a-z0-9]+(-[a-z0-9]+)*$`, unique across the suite, identical to the file
  stem. Prefix with the fixture language (`ts-`, `py-`, `rust-`, `go-`, `java-`,
  `c-`, `cpp-`, `ruby-`, `php-`, `polyglot-`).
- `repository`: path of the fixture relative to the workspace root. The runner never
  works inside it; it copies the fixture to a scratch directory first (see below).
- `category`: the four values of `TaskCategory` in `crates/core` (snake_case). The
  category also selects the agent's context budget, so pick it by the real shape of
  the change, not by how hard the prompt sounds: `localized_edit` is one file,
  `multi_file_change` is several existing files following an existing pattern,
  `complex_feature` needs new files or new structure, `investigation` must not edit.
- `languages`: lower-case tags used for filtering and per-language reporting
  (`typescript`, `tsx`, `python`, `rust`, `go`, `java`, `c`, `cpp`, `ruby`, `php`,
  `javascript`, `markdown`).
- `prompt`: what the user says in turn 1. Written like a real request, so it may
  name a function or a behaviour but it should not hand over the file path unless a
  user realistically would.
- `validation`: shell commands, run in order with `sh -c` from the scratch copy of
  the repository after the final turn. The task passes only if every command exits
  0. Commands may use shell syntax (`!`, pipes, `$(...)`). An empty list is only
  valid for `investigation` tasks. Every command must be known to work in the
  fixture: either it passes on the pristine fixture (a test suite the change must
  keep green) or it fails on the pristine fixture for exactly the reason the task
  exists (a grep for the new symbol, a behavioural probe of the new command). The
  comment block at the top of each task file records which is which.
- `expected_files`: fixture-relative paths a correct change touches. They drive
  retrieval recall and edit precision (below). A path that does not exist in the
  pristine fixture means the task expects the agent to create it; recall is
  computed over the paths that do exist.
- `requires_tools`: executables that must resolve on `PATH` for the task to run.
  If any is missing the runner records the sample as **skipped**, never as failed,
  and the report lists which tasks were skipped and why.
- `timeout_secs`: wall-clock budget for the agent's work on the task, all turns
  included. Validation commands have their own limit (the runtime's
  `ValidationConfig.timeout_secs`, 900 s by default).
- `[[turns]]`: optional follow-up turns, each with its own `prompt` and
  `expected_files`. Turns run sequentially in the same scratch copy and the same
  agent session, so turn 2 sees the conversation and edits of turn 1. Validation
  runs once, after the last turn, and should cover every turn's outcome.

## Task catalogue

| id                                  | category          | fixture      | tools             | notes                                                     |
| ----------------------------------- | ----------------- | ------------ | ----------------- | --------------------------------------------------------- |
| `ts-login-loading`                  | localized_edit    | ts-web-app   | node              | the canonical spinner task                                |
| `ts-footer-year`                    | localized_edit    | ts-web-app   | node              | one-constant edit; floor for retrieval                    |
| `ts-login-loading-smaller-spinner`  | localized_edit    | ts-web-app   | node              | multi-turn; turn 2 continues turn 1 on the same file      |
| `ts-login-loading-then-footer-year` | localized_edit    | ts-web-app   | node              | multi-turn; turn 2 is unrelated (re-retrieval)            |
| `ts-projects-list`                  | complex_feature   | ts-web-app   | node              | new hook + new component + App edit                       |
| `py-discount-bug-fix`               | localized_edit    | py-service   | python3           | makes the one red test green                              |
| `py-rename-apply-discount`          | multi_file_change | py-service   | python3           | rename across package and tests; grep + import validation |
| `py-explain-failing-test`           | investigation     | py-service   | python3           | read-only; no validation commands                         |
| `rust-whisper-command`              | multi_file_change | rust-cli     | cargo             | new sub-command following the documented pattern          |
| `rust-repeat-option`                | complex_feature   | rust-cli     | cargo             | first flag the parser accepts; touches the run path       |
| `go-delete-item`                    | multi_file_change | go-service   | go                | store method + handler + tests                            |
| `java-modulo-operator`              | multi_file_change | java-lib     | mvn, java         | not executable on the authoring machine                   |
| `c-vec-pop`                         | multi_file_change | c-lib        | make, cc          | header + impl + test                                      |
| `cpp-matrix-transpose`              | multi_file_change | cpp-lib      | make, c++         | header + impl + test                                      |
| `ruby-slug-accents`                 | localized_edit    | ruby-gem     | ruby              | stdlib-only transliteration; locale-proof check           |
| `php-cart-remove`                   | multi_file_change | php-app      | php, composer     | not executable on the authoring machine                   |
| `polyglot-web-api-investigation`    | investigation     | polyglot     | (none)            | read-only; three languages                                |
| `polyglot-python-format-price`      | localized_edit    | polyglot     | python3, node     | retrieval ambiguity: two `format_price` definitions       |
| `polyglot-version-bump`             | multi_file_change | polyglot     | cargo             | Rust constant + tests + Markdown banner                   |

19 tasks: 7 localized_edit, 8 multi_file_change, 2 complex_feature, 2 investigation
(the two multi-turn sequences are counted under localized_edit).

## Running the suite

The runner is the part of the workspace that implements this protocol; the
protocol itself is defined here so that results from different runners, machines
and dates are comparable.

1. **Pin everything.** Record the workspace commit, the model and provider, the
   agent configuration (or its hash), the runner version, and the versions of the
   toolchains (`node --version`, `python3 --version`, `cargo --version`, ...).
2. **Check tools.** For every distinct entry in `requires_tools`, run
   `command -v <tool>`. Tasks with a missing tool are skipped and listed in the
   report. On the authoring machine `cargo` lives in `~/.cargo/bin`, which is not on
   `PATH` by default; add it before running so the Rust tasks are not skipped.
3. **Check fixtures are pristine.** No `target/`, `__pycache__/`, `*.o`,
   `tests/test_vec`, `tests/test_matrix`, `Cargo.lock`, `node_modules/` or `vendor/`
   anywhere under `tests/repositories/`, and the baseline commands in the fixture
   table above give the stated results. A dirty fixture invalidates the run.
4. **For each task, for each sample `1..n`:**
   1. Copy the fixture to a fresh scratch directory
      (`cp -R tests/repositories/<name> "$SCRATCH/<task-id>/<sample>/"`). The agent
      and the validation commands only ever see this copy, so build artefacts and
      failed attempts never touch the fixture or other samples.
   2. Start a new agent session rooted at the copy and send the turn-1 prompt. For
      each `[[turns]]` entry, wait for the agent to finish, then send the next
      prompt in the same session. Enforce `timeout_secs` across all turns.
   3. After the last turn, run each `validation` command with `sh -c` from the
      copy, in order, stopping at the first non-zero exit. Record every command's
      exit code and duration.
   4. Record what the agent retrieved and what it changed (diff against the
      pristine fixture), plus steps, tokens and wall time.
   5. Delete the scratch directory.
5. **Aggregate** into `results/<run-id>/` (layout below) and write the report.

Keep the environment fixed across samples: same `PATH`, same locale (the
validation commands are written to work under the plain `C` locale as well as
UTF-8), no network except where a task cannot avoid it (`java-modulo-operator`
needs Maven to fetch JUnit once; `php-cart-remove` needs Composer to fetch PHPUnit
once). Samples of the same task should be run with the same sampling settings;
if the provider supports seeds, vary the seed per sample and record it.

## Scoring

**Task pass.** A sample passes when every validation command exits 0 within the
limits. Timeouts, crashes, a blocked agent and a missing tool are distinct outcomes
(`timeout`, `error`, `blocked`, `skipped`); only `pass` and `fail` enter the pass
rate, and the others are reported alongside it.

**Investigation tasks** have no commands. A sample of an investigation task passes
the hard check only if the scratch copy is byte-identical to the fixture after the
run. Answer quality is scored separately against the facts listed in the comment
block of the task file (for example, that `fetchProducts` issues
`GET {API_BASE}/products` and that `handle_request("/products")` answers it), by a
rubric or an LLM judge, and reported as its own column, never folded into the pass
rate.

**Retrieval recall** for a task sample is the fraction of `expected_files` (that
exist in the pristine fixture) that the agent retrieved into context before its
first edit of the turn. For multi-turn tasks it is computed per turn with that
turn's `expected_files`, which is how the suite measures whether the agent
re-retrieves after an unrelated follow-up (`ts-login-loading-then-footer-year`) and
whether it keeps context for a continuation (`ts-login-loading-smaller-spinner`).

**Edit precision** is the fraction of files the agent modified or created that are
in `expected_files` (any turn). Touching `web/index.mjs` in
`polyglot-python-format-price` costs precision without failing validation; that is
intentional, because the task exists to measure disambiguation.

**Cost** is reported as steps, input and output tokens, and wall-clock seconds per
sample, so that a configuration that passes more often but costs twice as much is
visible as such.

## Reporting: sample counts and confidence intervals

Every pass rate is published as `k/n` together with a 95% confidence interval.
A bare percentage is never reported.

- **Sample counts.** Run at least `n = 5` samples per task for a smoke run and at
  least `n = 10` per task for any number that is compared against another
  configuration or published. Skipped tasks are excluded from the denominators and
  listed separately with the missing tool.
- **Per-task interval: Wilson score.** With `p = k / n` and `z = 1.96`:

  ```
  centre = (p + z^2 / (2n)) / (1 + z^2 / n)
  half   = z * sqrt(p(1 - p) / n + z^2 / (4n^2)) / (1 + z^2 / n)
  CI     = [centre - half, centre + half]
  ```

  The Wilson interval behaves at the extremes (0/n and n/n) where the normal
  approximation does not. Reference points:

  | k/n   | rate | 95% Wilson CI  |
  | ----- | ---- | -------------- |
  | 0/10  | 0.00 | [0.00, 0.28]   |
  | 5/5   | 1.00 | [0.57, 1.00]   |
  | 7/10  | 0.70 | [0.40, 0.89]   |
  | 10/10 | 1.00 | [0.72, 1.00]   |
  | 14/20 | 0.70 | [0.48, 0.85]   |

  Read these before drawing conclusions: ten samples cannot tell 70% from 45% or
  from 88%.
- **Category and suite aggregates.** Report the pooled `k/n` with its Wilson
  interval and the per-task table underneath it. Pooled samples are not
  independent across tasks, so for category-level comparisons also give a
  bootstrap interval over tasks: resample the tasks of the category with
  replacement 10,000 times, average the per-task pass rates each time, and report
  the 2.5th and 97.5th percentiles.
- **Comparing two configurations.** Run both on the same task list, the same
  fixtures and the same `n`. Compute the per-task difference in pass rate, then the
  paired bootstrap over tasks (10,000 resamples) of the mean difference. Claim an
  improvement only when the 95% interval excludes zero, and always print the
  interval, not just the verdict. Report retrieval recall and cost the same way.
- **Retrieval queries.** Report mean recall@5, recall@10 and MRR over the queries
  with a bootstrap interval over queries, plus the per-kind breakdown (filename,
  symbol, natural language, user-visible string).

A summary table has these columns, in this order:
`task | category | n | pass | fail | timeout | error | skipped | pass rate | 95% CI | mean recall | mean precision | median steps | median wall s`.

## Results layout

```
results/<run-id>/
  run.json        metadata: commit, model, provider, config hash, toolchain versions,
                  start/end time, task list, n per task, skipped tasks with reasons
  samples.jsonl   one object per task x sample x turn
  summary.md      the tables described above
```

Each `samples.jsonl` line carries at least: `task_id`, `sample`, `turn`, `outcome`
(`pass|fail|timeout|error|blocked|skipped`), `validation` (list of
`{command, exit_code, duration_ms}`; empty for investigation tasks and for turns
before the last), `unchanged` (investigation hard check), `retrieved_files`,
`expected_files`, `recall`, `files_touched`, `precision`, `steps`, `tokens_in`,
`tokens_out`, `wall_secs`, `seed`, `error`.

`<run-id>` is `<date>-<model>-<short config hash>` so that runs sort
chronologically and the configuration is visible in the directory name.

## Retrieval queries

`retrieval/queries.jsonl` holds one object per line with exactly three keys:

```json
{"repository": "tests/repositories/go-service", "query": "error response \"item not found\"", "relevant_files": ["internal/handlers/handlers.go", "internal/handlers/handlers_test.go"]}
```

- `repository` is the fixture, relative to the workspace root.
- `query` is what a user would type. Four kinds are mixed, in roughly equal
  measure, for every fixture:
  - filename queries (`LoginButton.tsx`, `cli.rs`),
  - symbol queries (`useAuth`, `ErrNotFound`, `Matrix::multiply`),
  - natural-language functionality queries (`where are unknown sub-commands turned into an error`),
  - user-visible string queries, where the literal string is wrapped in double
    quotes inside the query and appears verbatim in at least one relevant file
    (`error message "Division by zero"`).
- `relevant_files` are fixture-relative paths that all exist. They are the files a
  reasonable engineer would open for that query: the definition for a symbol, the
  source and the mirrored test for a string that appears in both, both definitions
  for a deliberately ambiguous symbol (`format_price` in `polyglot`).

Scoring: for each query, rank the repository's files with the retriever under test
and compute recall@k (fraction of `relevant_files` in the top k) for k = 5 and 10,
and the reciprocal rank of the first relevant file. The fixtures are small, so
recall@10 is close to a ceiling test; recall@5 and MRR are the discriminating
numbers. The suite currently has 44 queries across all ten fixtures.

## Adding a task or query

1. Read the fixture first and refer to real paths, symbols and strings.
2. Pick the fixture's own test command for `validation`, and add at least one check
   that fails on the pristine fixture for the task's reason (a grep for the new
   symbol, a behavioural probe). Run every command from the fixture directory and
   record the pristine outcome in the comment block at the top of the task file.
3. Never make a validation command depend on an unrelated known failure (the
   `py-service` discount bug is the example); choose grep or import checks or a
   narrower test target instead.
4. Keep prompts free of double quotes inside TOML basic strings, or use TOML
   literal strings (`'''...'''`) for prompts and commands that mix quote styles.
5. Run the lint checks before committing: every TOML parses; `id` equals the file
   stem and is unique; `category` is one of the four values; `repository` exists;
   every `expected_files` and `relevant_files` entry exists (except files a task
   creates, which must be called out in the task's comment); every quoted string in
   a string query greps in a relevant file; and the fixtures are clean afterwards
   (`make clean` in `c-lib`, `cpp-lib` and `polyglot`; remove `target/` and
   `Cargo.lock` in `rust-cli`; remove `__pycache__`).

## Environment notes

- **Node >= 21 treats the positional argument of `node --test` as a glob**, so a
  bare directory such as `src/__tests__/` resolves to its `index.js`; `ts-web-app`
  ships one that imports every `*.test.mjs`. New test files in that fixture must be
  added to `src/__tests__/index.js`. `polyglot` uses `node --test web/*.test.mjs`
  for the same reason.
- **`cargo` is offline-capable** for these fixtures (no dependencies); the tasks pass
  `--offline` so a sandbox without network still works. Both Rust fixtures carry an
  empty `[workspace]` table so they do not join the enclosing Cargo workspace.
  Never run `cargo` at the workspace root as part of a benchmark.
- **Locale.** The authoring machine runs with `LC_CTYPE=C`; the Ruby check in
  `ruby-slug-accents` therefore builds its accented input from code points instead of
  a UTF-8 literal. Prefer a UTF-8 locale for agents, but do not rely on it in
  validation commands.
- **Toolchain gaps** on the authoring machine: no JDK/Maven, no PHP/Composer. The
  `java-lib` and `php-app` tasks were reviewed by reading and their greps were
  verified; `mvn -q test` and `composer test` have not been executed here.
- **Artefacts.** `make test` leaves object files and test binaries, `cargo` leaves
  `target/` and `Cargo.lock`, Python leaves `__pycache__`. Running on scratch copies
  avoids the problem entirely; if you run a command in place, clean up before the
  next run (`tests/repositories/.gitignore` lists the usual suspects).

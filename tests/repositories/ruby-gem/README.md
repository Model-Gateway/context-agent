# textkit

A tiny Ruby gem with two text helpers, used as a test fixture for Context.

- `TextKit::Slug.generate(text)` – URL-safe slugs (`"Hello, World!"` → `"hello-world"`).
- `TextKit::Truncate.call(text, max)` – shorten text with an omission marker.

Tests use the minitest that ships with Ruby and need no gems:

```sh
ruby -Ilib -Itest -e 'Dir["test/*_test.rb"].each { |f| require File.expand_path(f) }'
```

(`rake test` works too when rake is installed.)

## Known issues

- `Slug.generate` drops accented characters instead of transliterating them:
  `"Crème Brûlée"` currently becomes `"crme-brle"`.

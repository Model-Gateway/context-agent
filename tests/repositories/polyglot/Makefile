# Polyglot monorepo fixture.
#
#   make test        Python + JavaScript suites (Rust skipped for speed)
#   make test-rust   build and test the Rust `tools` crate
#   make clean       remove caches and build output

CARGO ?= $(HOME)/.cargo/bin/cargo

.PHONY: test test-py test-js test-rust clean

test: test-py test-js

test-py:
	python3 -m unittest discover -s services/api -p 'test_*.py'

test-js:
	node --test web/*.test.mjs

test-rust:
	$(CARGO) test --manifest-path tools/Cargo.toml

clean:
	find . -name '__pycache__' -type d -prune -exec rm -rf {} +
	find . -name '*.pyc' -delete
	rm -rf tools/target tools/Cargo.lock

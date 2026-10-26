# py-service

A tiny order-pricing service used as a test fixture for Context. It has
no third-party dependencies; everything runs on the standard library.

## Layout

- `svc/config.py`   – runtime settings (currency, tax rate, default discount).
- `svc/models.py`   – the `Order` and `LineItem` dataclasses.
- `svc/pricing.py`  – pure pricing helpers (discounts, tax, rounding).
- `svc/handlers.py` – request handlers that compose models and pricing.
- `tests/`          – `unittest` suites, one module per source module.

## Running the tests

```sh
python3 -m unittest discover -s tests -v
```

## Known issues

- The percentage discount helper in `svc/pricing.py` is wrong: it treats the
  percentage as an absolute amount. `tests/test_pricing.py` has a test that
  fails because of this.

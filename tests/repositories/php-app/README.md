# php-app

A small shopping-cart domain model used as a test fixture for Context.

- `src/Money.php`    – immutable value object (integer cents + currency).
- `src/Discount.php` – percentage discount applied to a `Money`.
- `src/Cart.php`     – line items, totals and discount application.
- `tests/CartTest.php` – PHPUnit tests.

```sh
composer install
composer test   # runs phpunit
```

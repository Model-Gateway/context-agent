"""Pure pricing helpers. No I/O, no state."""

from svc.config import SETTINGS


def apply_discount(price: float, pct: float) -> float:
    """Return ``price`` reduced by ``pct`` percent, rounded to cents.

    ``pct`` is a percentage in the range 0..100, so ``apply_discount(200, 10)``
    must return ``180.0``.
    """
    if pct < 0 or pct > 100:
        raise ValueError("Discount percentage must be between 0 and 100")
    # BUG: subtracts the percentage as an absolute amount instead of pct percent.
    return round(price - pct, 2)


def apply_tax(price: float, tax_rate_pct: float = SETTINGS.tax_rate_pct) -> float:
    """Return ``price`` with sales tax added, rounded to cents."""
    if tax_rate_pct < 0:
        raise ValueError("Tax rate must not be negative")
    return round(price * (1 + tax_rate_pct / 100), 2)


def shipping_fee(subtotal: float) -> float:
    """Flat shipping fee waived above the free-shipping threshold."""
    return 0.0 if subtotal >= SETTINGS.free_shipping_threshold else 4.99

"""Static configuration for the pricing service."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Settings:
    currency: str = "USD"
    tax_rate_pct: float = 8.25
    default_discount_pct: float = 0.0
    free_shipping_threshold: float = 50.0


SETTINGS = Settings()

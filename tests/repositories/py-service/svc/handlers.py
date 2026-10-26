"""Request handlers. These are plain functions so they are easy to test."""

from typing import Dict

from svc.config import SETTINGS
from svc.models import Order
from svc.pricing import apply_discount, apply_tax, shipping_fee


def checkout(order: Order) -> Dict[str, float]:
    """Compute the final charge breakdown for an order."""
    subtotal = order.subtotal
    discounted = apply_discount(subtotal, order.discount_pct)
    taxed = apply_tax(discounted)
    shipping = shipping_fee(discounted)
    return {
        "subtotal": subtotal,
        "discounted": discounted,
        "tax": round(taxed - discounted, 2),
        "shipping": shipping,
        "total": round(taxed + shipping, 2),
    }


def quote(order: Order) -> str:
    """Human-readable one-line quote used by the CLI and email templates."""
    breakdown = checkout(order)
    return "Order {id}: {total:.2f} {currency} ({count} items)".format(
        id=order.order_id,
        total=breakdown["total"],
        currency=SETTINGS.currency,
        count=len(order.items),
    )

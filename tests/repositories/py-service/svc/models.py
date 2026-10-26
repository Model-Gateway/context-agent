"""Domain models for orders."""

from dataclasses import dataclass, field
from typing import List


@dataclass
class LineItem:
    sku: str
    unit_price: float
    quantity: int = 1

    @property
    def subtotal(self) -> float:
        return round(self.unit_price * self.quantity, 2)


@dataclass
class Order:
    order_id: str
    customer_email: str
    items: List[LineItem] = field(default_factory=list)
    discount_pct: float = 0.0

    def add_item(self, item: LineItem) -> None:
        self.items.append(item)

    @property
    def subtotal(self) -> float:
        return round(sum(item.subtotal for item in self.items), 2)

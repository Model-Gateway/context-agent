import unittest

from svc.handlers import checkout, quote
from svc.models import LineItem, Order


def make_order(discount_pct: float = 0.0) -> Order:
    order = Order(order_id="A-100", customer_email="pat@example.com", discount_pct=discount_pct)
    order.add_item(LineItem(sku="WIDGET", unit_price=20.0, quantity=2))
    order.add_item(LineItem(sku="GADGET", unit_price=15.0))
    return order


class HandlerTests(unittest.TestCase):
    def test_checkout_without_discount(self):
        breakdown = checkout(make_order())
        self.assertEqual(breakdown["subtotal"], 55.0)
        self.assertEqual(breakdown["discounted"], 55.0)
        self.assertEqual(breakdown["shipping"], 0.0)
        self.assertEqual(breakdown["total"], 59.54)

    def test_quote_mentions_order_id_and_total(self):
        text = quote(make_order())
        self.assertIn("Order A-100", text)
        self.assertIn("59.54 USD", text)
        self.assertIn("2 items", text)


if __name__ == "__main__":
    unittest.main()

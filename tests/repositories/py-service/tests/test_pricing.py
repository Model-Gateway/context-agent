import unittest

from svc.pricing import apply_discount, apply_tax, shipping_fee


class PricingTests(unittest.TestCase):
    def test_zero_discount_is_identity(self):
        self.assertEqual(apply_discount(100.0, 0), 100.0)

    def test_ten_percent_off(self):
        # 10% off 200.00 is 180.00. Fails while the bug in pricing.py is present.
        self.assertEqual(apply_discount(200.0, 10), 180.0)

    def test_rejects_negative_percentage(self):
        with self.assertRaises(ValueError):
            apply_discount(100.0, -5)

    def test_rejects_percentage_above_100(self):
        with self.assertRaises(ValueError):
            apply_discount(100.0, 101)

    def test_tax_is_added(self):
        self.assertEqual(apply_tax(100.0, 8.25), 108.25)

    def test_shipping_is_free_above_threshold(self):
        self.assertEqual(shipping_fee(50.0), 0.0)
        self.assertEqual(shipping_fee(49.99), 4.99)


if __name__ == "__main__":
    unittest.main()

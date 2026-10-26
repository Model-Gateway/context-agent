import json
import unittest

import app


class FormatPriceTests(unittest.TestCase):
    def test_whole_and_fractional_dollars(self):
        self.assertEqual(app.format_price(1234), "$12.34")
        self.assertEqual(app.format_price(100), "$1.00")

    def test_pads_cents_to_two_digits(self):
        self.assertEqual(app.format_price(5), "$0.05")
        self.assertEqual(app.format_price(0), "$0.00")

    def test_negative_amounts_keep_sign_before_symbol(self):
        self.assertEqual(app.format_price(-250), "-$2.50")

    def test_no_thousands_separator(self):
        # The JS format_price groups thousands; this one deliberately does not.
        self.assertEqual(app.format_price(123456), "$1234.56")

    def test_rejects_non_integers(self):
        with self.assertRaises(TypeError):
            app.format_price(12.34)
        with self.assertRaises(TypeError):
            app.format_price("1234")
        with self.assertRaises(TypeError):
            app.format_price(True)


class ListProductsTests(unittest.TestCase):
    def test_returns_every_product_with_formatted_price(self):
        products = app.list_products()
        self.assertEqual(len(products), len(app.PRODUCTS))
        for product in products:
            self.assertEqual(
                set(product), {"id", "name", "price_cents", "price"}
            )
            self.assertEqual(product["price"], app.format_price(product["price_cents"]))

    def test_is_json_serialisable(self):
        json.dumps(app.list_products())


class HandleRequestTests(unittest.TestCase):
    def test_health(self):
        self.assertEqual(app.handle_request("/health"), (200, {"status": "ok"}))

    def test_products_shape_matches_architecture_doc(self):
        status, body = app.handle_request("/products")
        self.assertEqual(status, 200)
        self.assertEqual(list(body), ["products"])
        self.assertEqual(body["products"], app.list_products())

    def test_single_product(self):
        status, body = app.handle_request("/products/2")
        self.assertEqual(status, 200)
        self.assertEqual(body["product"]["name"], "Pen")
        self.assertEqual(body["product"]["price"], "$1.99")

    def test_unknown_product_is_404(self):
        status, body = app.handle_request("/products/999")
        self.assertEqual(status, 404)
        self.assertEqual(body["error"], "not found")

    def test_non_numeric_product_id_is_404(self):
        status, _ = app.handle_request("/products/abc")
        self.assertEqual(status, 404)

    def test_unknown_path_is_404(self):
        status, body = app.handle_request("/nope")
        self.assertEqual(status, 404)
        self.assertEqual(body, {"error": "not found", "path": "/nope"})


if __name__ == "__main__":
    unittest.main()

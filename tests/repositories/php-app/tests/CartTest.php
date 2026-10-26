<?php

declare(strict_types=1);

namespace App\Tests;

use App\Cart;
use App\Discount;
use App\Money;
use InvalidArgumentException;
use PHPUnit\Framework\TestCase;

final class CartTest extends TestCase
{
    public function testEmptyCartHasZeroTotal(): void
    {
        $cart = new Cart();

        self::assertTrue($cart->isEmpty());
        self::assertSame(0, $cart->total()->cents);
    }

    public function testSubtotalSumsLineItems(): void
    {
        $cart = new Cart();
        $cart->add('WIDGET', Money::fromDollars(20.00), 2);
        $cart->add('GADGET', Money::fromDollars(15.00));

        self::assertSame(5500, $cart->subtotal()->cents);
        self::assertSame('55.00 USD', $cart->subtotal()->format());
    }

    public function testAddingSameSkuMergesQuantity(): void
    {
        $cart = new Cart();
        $cart->add('WIDGET', Money::fromDollars(10.00));
        $cart->add('WIDGET', Money::fromDollars(10.00), 2);

        self::assertSame(3000, $cart->subtotal()->cents);
    }

    public function testDiscountReducesTotal(): void
    {
        $cart = new Cart();
        $cart->add('WIDGET', Money::fromDollars(200.00));
        $cart->applyDiscount(new Discount(10));

        self::assertSame(18000, $cart->total()->cents);
    }

    public function testRejectsZeroQuantity(): void
    {
        $this->expectException(InvalidArgumentException::class);

        (new Cart())->add('WIDGET', Money::fromDollars(1.00), 0);
    }
}

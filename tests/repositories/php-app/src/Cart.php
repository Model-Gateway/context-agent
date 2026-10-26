<?php

declare(strict_types=1);

namespace App;

use InvalidArgumentException;

/**
 * A shopping cart keyed by SKU. Adding the same SKU twice merges quantities.
 */
final class Cart
{
    /** @var array<string, array{price: Money, quantity: int}> */
    private array $items = [];

    private ?Discount $discount = null;

    public function add(string $sku, Money $unitPrice, int $quantity = 1): void
    {
        if ($quantity < 1) {
            throw new InvalidArgumentException('Quantity must be at least 1');
        }
        if (isset($this->items[$sku])) {
            $this->items[$sku]['quantity'] += $quantity;

            return;
        }
        $this->items[$sku] = ['price' => $unitPrice, 'quantity' => $quantity];
    }

    public function applyDiscount(Discount $discount): void
    {
        $this->discount = $discount;
    }

    public function subtotal(): Money
    {
        $sum = new Money(0);
        foreach ($this->items as $item) {
            $sum = $sum->add($item['price']->multiply($item['quantity']));
        }

        return $sum;
    }

    public function total(): Money
    {
        $subtotal = $this->subtotal();

        return $this->discount === null ? $subtotal : $this->discount->apply($subtotal);
    }

    public function isEmpty(): bool
    {
        return $this->items === [];
    }
}

<?php

declare(strict_types=1);

namespace App;

use InvalidArgumentException;

/**
 * A percentage discount in the range 0..100.
 */
final class Discount
{
    public function __construct(public readonly float $percentage)
    {
        if ($percentage < 0 || $percentage > 100) {
            throw new InvalidArgumentException('Discount percentage must be between 0 and 100');
        }
    }

    public function apply(Money $amount): Money
    {
        $remaining = (int) round($amount->cents * (1 - $this->percentage / 100));

        return new Money($remaining, $amount->currency);
    }
}

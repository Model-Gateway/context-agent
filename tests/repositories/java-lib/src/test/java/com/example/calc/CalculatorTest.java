package com.example.calc;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

public class CalculatorTest {

    private static final double EPSILON = 1e-9;

    @Test
    public void addsTwoNumbers() {
        assertEquals(5.0, Calculator.add(2, 3), EPSILON);
    }

    @Test
    public void subtractsTwoNumbers() {
        assertEquals(-1.0, Calculator.subtract(2, 3), EPSILON);
    }

    @Test
    public void multipliesTwoNumbers() {
        assertEquals(6.0, Calculator.multiply(2, 3), EPSILON);
    }

    @Test
    public void dividesTwoNumbers() {
        assertEquals(2.5, Calculator.divide(5, 2), EPSILON);
    }

    @Test(expected = ArithmeticException.class)
    public void divisionByZeroThrows() {
        Calculator.divide(1, 0);
    }

    @Test
    public void parserEvaluatesInfixExpressions() {
        assertEquals(7.0, Parser.evaluate("4 + 3"), EPSILON);
        assertEquals(12.0, Parser.evaluate(" 4 * 3 "), EPSILON);
    }

    @Test(expected = IllegalArgumentException.class)
    public void parserRejectsUnknownOperator() {
        Parser.evaluate("4 ^ 3");
    }
}

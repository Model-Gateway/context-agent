package com.example.calc;

/**
 * Evaluates expressions of the form {@code "<number> <op> <number>"} where
 * {@code op} is one of {@code + - * /}. Whitespace around tokens is ignored.
 */
public final class Parser {

    private Parser() {
    }

    public static double evaluate(String expression) {
        if (expression == null || expression.trim().isEmpty()) {
            throw new IllegalArgumentException("Expression must not be empty");
        }
        String[] tokens = expression.trim().split("\\s+");
        if (tokens.length != 3) {
            throw new IllegalArgumentException("Expected '<number> <op> <number>' but got: " + expression);
        }
        double left = parseNumber(tokens[0]);
        double right = parseNumber(tokens[2]);
        switch (tokens[1]) {
            case "+":
                return Calculator.add(left, right);
            case "-":
                return Calculator.subtract(left, right);
            case "*":
                return Calculator.multiply(left, right);
            case "/":
                return Calculator.divide(left, right);
            default:
                throw new IllegalArgumentException("Unsupported operator: " + tokens[1]);
        }
    }

    private static double parseNumber(String token) {
        try {
            return Double.parseDouble(token);
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException("Not a number: " + token, e);
        }
    }
}

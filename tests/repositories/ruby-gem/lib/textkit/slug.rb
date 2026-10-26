# frozen_string_literal: true

module TextKit
  # Builds URL-safe slugs from free text.
  class Slug
    DEFAULT_SEPARATOR = "-"

    # Generate a slug: lower-case ASCII letters and digits joined by +separator+.
    #
    #   Slug.generate("Hello, World!")  # => "hello-world"
    #   Slug.generate("a  b", separator: "_")  # => "a_b"
    #
    # Note: non-ASCII characters are currently stripped rather than
    # transliterated, so "Crème Brûlée" becomes "crme-brle".
    def self.generate(text, separator: DEFAULT_SEPARATOR)
      raise ArgumentError, "text must be a String" unless text.is_a?(String)

      text
        .downcase
        .gsub(/[^a-z0-9]+/, separator)
        .gsub(/\A#{Regexp.escape(separator)}+|#{Regexp.escape(separator)}+\z/, "")
    end
  end
end

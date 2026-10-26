# frozen_string_literal: true

module TextKit
  # Shortens text to a maximum length, appending an omission marker.
  class Truncate
    DEFAULT_OMISSION = "..."

    # Truncate +text+ so the result is at most +max+ characters long.
    #
    #   Truncate.call("The quick brown fox", 10)  # => "The qui..."
    def self.call(text, max, omission: DEFAULT_OMISSION)
      raise ArgumentError, "max must be non-negative" if max.negative?
      return text if text.length <= max
      return omission[0, max] if max <= omission.length

      text[0, max - omission.length] + omission
    end
  end
end

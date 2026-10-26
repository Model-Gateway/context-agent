# frozen_string_literal: true

require "minitest/autorun"
require "textkit"

class TruncateTest < Minitest::Test
  def test_short_text_is_unchanged
    assert_equal "short", TextKit::Truncate.call("short", 10)
  end

  def test_long_text_is_cut_with_omission
    assert_equal "The qui...", TextKit::Truncate.call("The quick brown fox", 10)
  end

  def test_custom_omission
    assert_equal "The quick~", TextKit::Truncate.call("The quick brown fox", 10, omission: "~")
  end

  def test_tiny_max_returns_partial_omission
    assert_equal "..", TextKit::Truncate.call("anything", 2)
  end

  def test_negative_max_raises
    assert_raises(ArgumentError) { TextKit::Truncate.call("x", -1) }
  end
end

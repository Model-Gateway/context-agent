# frozen_string_literal: true

require "minitest/autorun"
require "textkit"

class SlugTest < Minitest::Test
  def test_basic_slug
    assert_equal "hello-world", TextKit::Slug.generate("Hello, World!")
  end

  def test_collapses_runs_of_separators
    assert_equal "a-b-c", TextKit::Slug.generate("a   b -- c")
  end

  def test_custom_separator
    assert_equal "a_b", TextKit::Slug.generate("a  b", separator: "_")
  end

  def test_strips_leading_and_trailing_separators
    assert_equal "tidy", TextKit::Slug.generate("  --tidy!! ")
  end

  def test_rejects_non_strings
    assert_raises(ArgumentError) { TextKit::Slug.generate(nil) }
  end
end

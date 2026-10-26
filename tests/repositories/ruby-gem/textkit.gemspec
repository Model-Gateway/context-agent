# frozen_string_literal: true

require_relative "lib/textkit/version"

Gem::Specification.new do |spec|
  spec.name          = "textkit"
  spec.version       = TextKit::VERSION
  spec.authors       = ["Context fixtures"]
  spec.email         = ["fixtures@example.com"]
  spec.summary       = "Small text helpers: slugs and truncation."
  spec.description   = "Fixture gem used by the Context test-suite. No runtime dependencies."
  spec.homepage      = "https://example.invalid/textkit"
  spec.license       = "MIT"
  spec.required_ruby_version = ">= 2.6"

  spec.files         = Dir["lib/**/*.rb"]
  spec.require_paths = ["lib"]
end

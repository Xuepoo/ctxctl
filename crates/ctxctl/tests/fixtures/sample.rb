require 'json'
require_relative 'helpers'
require './local'

# A user entity.
class User
  # Says hello.
  def greet(name)
    "hi #{name}"
  end

  def self.build
    User.new
  end
end

# Computes a sum.
def add(a, b)
  a + b
end

module Utils
  def normalize(x)
    x.to_f
  end
end

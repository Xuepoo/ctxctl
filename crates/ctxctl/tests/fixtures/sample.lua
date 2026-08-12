local json = require "json"
local helpers = require("./helpers")

-- A counter.
local Counter = {}
Counter.__index = Counter

function Counter.new()
  return setmetatable({ n = 0 }, Counter)
end

-- Adds two numbers.
local function add(a, b)
  return a + b
end

function Counter:increment()
  self.n = self.n + 1
end

local MAX_RETRIES = 3

//! Fixture module used by ctxctl integration tests.

/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn norm(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

pub const ANSWER: i32 = 42;

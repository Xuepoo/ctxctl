"""Fixture module used by ctxctl integration tests."""

import math


class Point:
    """A point in 2D space."""

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y

    def norm(self) -> float:
        return math.sqrt(self.x * self.x + self.y * self.y)


def add(a: int, b: int) -> int:
    """Adds two integers."""
    return a + b

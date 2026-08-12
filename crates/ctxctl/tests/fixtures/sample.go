// Package fixture is used by ctxctl integration tests.
package fixture

// Point is a 2D point.
type Point struct {
	X float64
	Y float64
}

// Norm returns the distance from the origin.
func (p *Point) Norm() float64 {
	return p.X*p.X + p.Y*p.Y
}

// Add sums two integers.
func Add(a, b int) int {
	return a + b
}

const MaxRetries = 3

package main

import "strings"

// Inferred pure: basic parameters, one result, and a body that only computes.
func add(a, b int) int { return a + b }

// Pure transitively — `add` is pure, so this is too.
func double(a int) int { return add(a, a) }

// Pure through the stdlib table: strings.TrimSpace is in purity.pureStdlib.
func trimmed(s string) string { return strings.TrimSpace(s) + "!" }

type point struct{ x, y int }

// A struct whose fields are all basic counts as basic for a parameter.
func norm(p point) int { return p.x*p.x + p.y*p.y }

func main() {
	strings.ToLower("x")
	add(1, 2)
	double(3)
	trimmed(" x ")
	norm(point{1, 2})
}

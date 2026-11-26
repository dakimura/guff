package bad

type cfg struct {
	a int
	b int
}

// distinct compares two *different* fields to the same constant. This is not
// redundant and must not be flagged (regression: selector operands used to
// collapse to "_", making the two comparisons look identical).
func distinct(c *cfg) bool {
	return c.a == 0 && c.b == 0
}

// identical compares the *same* field twice — genuinely redundant.
func identical(c *cfg) bool {
	return c.a == 0 && c.a == 0
}

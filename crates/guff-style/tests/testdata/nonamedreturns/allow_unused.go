package nonamedreturnsallow

// Allowed: documentation-style named return, never referenced, explicit return.
func explicitReturn(a, b int) (sum int) {
	return a + b
}

// Reported: assigned in the body.
func assigned(a, b int) (sum int) { // want
	sum = a + b
	return sum
}

// Reported: naked return.
func nakedReturn() (sum int) { // want
	return
}

// Allowed: underscore skipped.
func underscoreResult() (_ int) {
	return 1
}

package r

// Hidden lives in r's test variant, and r has no external test package — so
// nothing in this load may see it.
func Hidden() int { return 1 }

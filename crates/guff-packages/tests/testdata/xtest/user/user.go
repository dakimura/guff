package user

import "example.com/xt/r"

// r.Hidden is test-variant-only; a seed that carried it here would make this
// package type-check, which Go does not.
func Use() int { return r.Hidden() }

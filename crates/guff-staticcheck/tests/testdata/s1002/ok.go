package main

func f(x bool) {
	if x {
		_ = x
	}
	if !x {
		_ = x
	}
}

// Named bool types used as enums must not trigger S1002 (honnef bias).
type TokenSource bool

const TokenSourceAPI TokenSource = true

func namedBoolEnum(source TokenSource) bool {
	return source == TokenSourceAPI
}

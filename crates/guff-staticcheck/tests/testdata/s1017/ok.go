package main

import "strings"

func f(s, prefix string) {
	if strings.HasPrefix(s, prefix) {
		s = s
	}
}

// An `if` that has an `else` is skipped, and upstream records its `Else` node
// so that an `else if` — which has no else of its own and otherwise looks like
// the reportable shape — is skipped as well:
//
//	if ifstmt.Else != nil { seen[ifstmt.Else] = struct{}{}; return }
//	if _, ok := seen[ifstmt]; ok { return }
//
// Neither arm below is reported. boundary's internal/cmd/commands/server writes
// exactly this and guff reported the second one.
func elseIf(s string) string {
	if strings.HasPrefix(s, "int-") {
		s = strings.TrimPrefix(s, "int-")
	} else if strings.HasPrefix(s, "dev-") {
		s = strings.TrimPrefix(s, "dev-")
	}
	return s
}

// The same, with a trailing else. The middle arm is still an else-if.
func elseIfElse(s string) string {
	if strings.HasPrefix(s, "a-") {
		s = strings.TrimPrefix(s, "a-")
	} else if strings.HasPrefix(s, "b-") {
		s = strings.TrimPrefix(s, "b-")
	} else {
		s = s + "!"
	}
	return s
}

// An `if` with an else of its own.
func withElse(s string) string {
	if strings.HasPrefix(s, "x-") {
		s = strings.TrimPrefix(s, "x-")
	} else {
		s = s + "!"
	}
	return s
}

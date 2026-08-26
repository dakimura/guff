package main

import "strings"

func f(a, b string) bool { return strings.ToLower(a) == strings.ToLower(b) }

// The `!=` spelling. Until 2026-08-27 the fixture held only `==`, so guff's
// message for this one — and the negation its fix has to add — went unmeasured.
// `ToLower`, not `ToUpper`: the unit-test harness stubs `strings` with the
// functions the fixtures use, and only the operator matters here.
func g(a, b string) bool { return strings.ToLower(a) != strings.ToLower(b) }

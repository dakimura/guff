package global

import "math/rand"

// Every function draws from math/rand (G404) and differs only in the
// directive after it; the golden cases run it under different
// config.global settings.

func a() int { return rand.Int() } // #nosec
func b() int { return rand.Int() } // #nosec G404
func c() int { return rand.Int() } // #nosec G404 -- test only
func d() int { return rand.Int() } // #nosec -- test only
func e() int { return rand.Int() } //gosec:disable G404 -- test only
func f() int { return rand.Int() } // #falsepositive G404 -- ok
func g() int { return rand.Int() } // #dontanalyze G404 -- ok
func h() int { return rand.Int() } // #nosec G404 --
func i() int { return rand.Int() }

var _ = []func() int{a, b, c, d, e, f, g, h, i}

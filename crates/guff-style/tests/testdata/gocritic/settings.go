// Package gocriticsettings is the fixture for go-critic's *selector* keys —
// `enable-all` / `disable-all` / `enabled-checks` / `disabled-checks` /
// `enabled-tags` / `disabled-tags` — rather than for any one checker.
//
// It carries one finding from each of the tag classes the selectors
// discriminate on, because the default set is defined by tags:
// `isEnabledByDefaultGoCriticChecker` keeps a checker only if it has none of
// experimental / opinionated / performance / security, which leaves the
// plain-style and plain-diagnostic ones.
//
//	singleCaseSwitch   style                     on by default
//	appendAssign       diagnostic                on by default
//	rangeValCopy       performance               off
//	paramTypeCombine   style + opinionated       off
//	emptyFallthrough   style + experimental      off
//
// Keeping it to five means `enable-all` stays readable: everything else the
// 100-plus checkers look for has to be absent, or the enable-all golden turns
// into a second copy of cases/gocritic.
package gocriticsettings

type big struct {
	a, b, c, d [8]int64
}

var pool []big

// singleCaseSwitch: a switch with exactly one case is an if.
func Single(v int) string {
	switch v {
	case 1:
		return "one"
	}
	return ""
}

// appendAssign: the result is assigned to a different slice than the one
// appended to.
type lists struct {
	positives []int
	negatives []int
}

func (l *lists) Add(x int) {
	l.positives = append(l.negatives, x)
}

// rangeValCopy: `v` is copied once per iteration, and `big` is over the
// 128-byte threshold.
func Copies() int64 {
	var total int64
	for _, v := range pool {
		total += v.a[0]
	}
	return total
}

// paramTypeCombine: `(a int, b int)` can be written `(a, b int)`.
func Combine(a int, b int) int {
	return a + b
}

// emptyFallthrough: the case body is nothing but the fallthrough.
func Fall(v int) string {
	switch v {
	case 1:
		fallthrough
	case 2:
		return "low"
	default:
		return "high"
	}
}

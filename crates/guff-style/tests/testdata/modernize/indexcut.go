// stringscut as x/tools v0.44 has it: `i := strings.Index(s, sep)` whose `i`
// is only ever tested for "found" / "not found" or used to slice `s` just
// before or just after the match becomes `strings.Cut` — or `Contains` when it
// is only tested. The three beats shapes lead; the rest walk every branch of
// upstream's `checkIdxUses` / `indexArgValid` / `isSliceIndexGuarded`,
// silent ones included, and the fix text (fresh names, per-scope suffixes,
// `IndexByte`'s byte argument) is gated byte for byte by compat/fix.
package indexcut

import (
	"bytes"
	"strings"
)

type rec struct{ f string }

func use(...any) {}

// beats metricbeat state_container: both halves, a 3-byte separator
func beforeAndAfter(cID string) (string, string) {
	split := strings.Index(cID, "://")
	if split != -1 {
		return cID[:split], cID[split+3:]
	}
	return "", ""
}

// beats packetbeat dns: if-init, the after half under the guard
func ifInitAfter(s string, m map[string]string) {
	if idx := strings.IndexByte(s, '.'); idx != -1 {
		m["tld"] = s[idx+1:]
	}
}

// beats packetbeat http: early return, then the before half
func earlyReturnBefore(c []byte) string {
	cs := string(c)
	s := strings.IndexByte(cs, ':')
	if s < 0 {
		return ""
	}
	return cs[:s]
}

// only tested: Contains
func onlyTested(s string) bool {
	i := strings.Index(s, "x")
	if i >= 0 {
		return true
	}
	return false
}

// every spelling of "not found"
func negatives(s string) {
	a := strings.Index(s, "a")
	b := strings.Index(s, "b")
	c := strings.Index(s, "c")
	d := strings.Index(s, "d")
	use(a == -1, -1 == b, 0 > c, d <= -1)
}

// every spelling of "found"
func nonnegatives(s string) {
	a := strings.Index(s, "a")
	b := strings.Index(s, "b")
	c := strings.Index(s, "c")
	use(a != -1, b > -1, -1 < c)
}

// `i > 0` says more than "found": silent
func strictlyPositive(s string) bool {
	i := strings.Index(s, "x")
	return i > 0
}

// `i` itself escapes: silent
func escapes(s string) int {
	i := strings.Index(s, "x")
	if i < 0 {
		return 0
	}
	return i
}

// `s` reassigned after the call: silent
func reassigned(s string) string {
	i := strings.Index(s, ":")
	s = "other"
	if i >= 0 {
		return s[:i]
	}
	return ""
}

// `&s` taken: silent
func addressTaken(s string) string {
	i := strings.Index(s, ":")
	use(&s)
	if i >= 0 {
		return s[:i]
	}
	return ""
}

// `s` is a field, not a local: silent
func fieldArg(r rec) bool {
	i := strings.Index(r.f, ":")
	return i >= 0
}

// bytes, with a []byte(sep) conversion
func bytesCut(b []byte) []byte {
	i := bytes.Index(b, []byte("::"))
	if i >= 0 {
		return b[:i]
	}
	return nil
}

// bytes.IndexByte with a variable byte
func bytesIndexByteVar(b []byte, c byte) bool {
	i := bytes.IndexByte(b, c)
	return i >= 0
}

// strings.IndexByte with a variable byte
func stringsIndexByteVar(s string, c byte) bool {
	i := strings.IndexByte(s, c)
	return i < 0
}

// one-byte separator, after half with no guard: silent
func unguardedOneByte(s string) string {
	i := strings.IndexByte(s, ':')
	return s[i+1:]
}

// three-byte separator, after half with no guard: no guard needed
func unguardedThreeBytes(s string) string {
	i := strings.Index(s, "://")
	return s[i+3:]
}

// the before half in the else of a negative check
func elseOfNegative(s string) string {
	i := strings.Index(s, "=")
	if i < 0 {
		return ""
	} else {
		return s[0:i]
	}
}

// var form
func varForm(s string) bool {
	var i = strings.Index(s, "v")
	return i != -1
}

// plain assignment to an existing variable: silent
func plainAssign(s string) bool {
	var i int
	i = strings.Index(s, "p")
	return i >= 0
}

// `ok` is taken and used later in the block: a fresh name
func nameTaken(s string) (string, bool) {
	ok := len(s) > 3
	i := strings.Index(s, "=")
	if i >= 0 {
		return s[:i], ok
	}
	return "", ok
}

// two fixes in one scope get suffixed names
func twoInScope(s, t string) (string, string) {
	i := strings.Index(s, "=")
	j := strings.Index(t, "=")
	var a, b string
	if i >= 0 {
		a = s[:i]
	}
	if j >= 0 {
		b = t[:j]
	}
	return a, b
}

// len(sep) with a variable separator
func lenSep(s, sep string) string {
	i := strings.Index(s, sep)
	if i >= 0 {
		return s[i+len(sep):]
	}
	return ""
}

// k + i
func constPlusI(s string) string {
	i := strings.Index(s, "ab")
	if i >= 0 {
		return s[2+i:]
	}
	return ""
}

// `i - 1`: upstream does not check the operator of `i + k`
func minusOne(s string) string {
	i := strings.Index(s, "x")
	if i >= 0 {
		return s[i-1:]
	}
	return ""
}

// the guard stops at a function literal
func closureGuard(s string) func() string {
	i := strings.IndexByte(s, ':')
	if i >= 0 {
		return func() string { return s[i+1:] }
	}
	return nil
}

// a slice with a max index: silent
func threeIndex(s []byte) []byte {
	i := bytes.IndexByte(s, ':')
	if i >= 0 {
		return s[:i:i]
	}
	return nil
}

// `i` in a parenthesised comparison: silent
func parenthesised(s string) bool {
	i := strings.Index(s, "x")
	return (i) >= 0
}

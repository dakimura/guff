package gosec_g115names

// How G115 *names* the two types, when the first (src, dst) pair the package
// reaches is a string range.
//
// go/ssa types a string range's value as `tRune` — `types.Universe.Lookup
// ("rune").Type()`, int32 under the name "rune" — not `Typ[Int32]`. And gosec
// caches its message per (src kind, dst kind) for the whole package
// (`overflowState.msgCache`, never reset between functions), so the first
// conversion reached names every later one of the same kinds, whatever the
// later one spelled. Every finding below reads "rune -> byte", including the
// `int32` parameter, because `constRange` comes first.
//
// Its sibling g115_cache.go is the same cache seen from the other side.

const rwx = "rwx"

type myString string

func constRange() byte {
	var b byte
	for _, c := range rwx {
		b = byte(c) // FINDING rune -> byte (the first: names the rest)
	}
	return b
}

func int32Param(r int32) byte { return byte(r) } // FINDING rune -> byte (cached, not "int32")

func litRange() byte {
	var b byte
	for _, c := range "abc" {
		b = byte(c) // FINDING rune -> byte
	}
	return b
}

func varRange(s string) byte {
	var b byte
	for _, c := range s {
		b = byte(c) // FINDING rune -> byte
	}
	return b
}

func namedRange(s myString) byte {
	var b byte
	for _, c := range s {
		b = byte(c) // FINDING rune -> byte
	}
	return b
}

func runeParam(r rune) byte { return byte(r) } // FINDING rune -> byte

func runeSlice(rs []rune) byte {
	var b byte
	for _, c := range rs {
		b = byte(c) // FINDING rune -> byte
	}
	return b
}

func closureRange(s string) func() byte {
	return func() (b byte) {
		for _, c := range s {
			b = byte(c) // FINDING rune -> byte
		}
		return b
	}
}

// A rune constant is folded, and a folded constant in range is safe.
func runeLit() byte {
	r := 'a'
	return byte(r) // silent
}

// A different pair gets its own message.
func otherPair(x uint8) int8 { return int8(x) } // FINDING uint8 -> int8

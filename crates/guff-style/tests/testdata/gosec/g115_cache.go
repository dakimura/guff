package gosec_g115cache

// The other side of g115_names.go: here the first conversion of each kind pair
// spells the *plain* name, and gosec's per-package message cache
// (`overflowState.msgCache`) hands that name to every later one — a string
// range reads "int32 -> byte", a `byte` parameter "uint8 -> int8".
//
// "First" is buildssa's `SrcFuncs` order: files as given, declarations in
// source order, each followed by its function literals. The functions are
// named against the alphabet on purpose — `zInt32` before `aRange` — so an
// order sorted by name gives different messages.

func zInt32(r int32) byte { return byte(r) } // FINDING int32 -> byte (the first)

func aRange(s string) (b byte) {
	for _, c := range s {
		b = byte(c) // FINDING int32 -> byte (cached, not "rune")
	}
	return b
}

func yRune(r rune) byte { return byte(r) } // FINDING int32 -> byte

// A string index is `Typ[Uint8]` in go/ssa (`tByte`), so it prints "uint8"…
func xStrIndex(s string, i int) int8 { return int8(s[i]) } // FINDING uint8 -> int8 (the first)

// …and so does the `byte` that comes after it.
func bByteParam(b byte) int8 { return int8(b) } // FINDING uint8 -> int8 (cached, not "byte")

func cByteSlice(bs []byte, i int) int8 { return int8(bs[i]) } // FINDING uint8 -> int8

// The destination is cached the same way: `byte(…)` first…
func wToByte(x int64) byte { return byte(x) } // FINDING int64 -> byte (the first)

// …so a later `uint8(…)` of the same source kind reads "byte".
func dToUint8(x int64) uint8 { return uint8(x) } // FINDING int64 -> byte (cached, not "uint8")

// A method and a literal inside it: the method is reached where it is
// declared, and its literal right after it, before `vAfter`.
type t struct{}

func (t) m(x uint16) func() int8 {
	return func() int8 { return int8(x) } // FINDING uint16 -> int8 (the first)
}

func vAfter(x uint16) int8 { return int8(x) } // FINDING uint16 -> int8

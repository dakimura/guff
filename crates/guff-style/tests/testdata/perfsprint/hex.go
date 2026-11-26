package p

import "fmt"

// `hex-format` is two cases upstream, not one, and they differ in both
// directions (catenacyber/perfsprint `analyzer.go`):
//
//	case isArray && … : refuses anything but an identifier
//	                    ("Doesn't support array literals") and appends `[:]`
//	case isSlice && … : takes any expression and appends nothing
//
// guff had a single "is this a byte sequence" predicate whose bool was read as
// "is it an array", so a `[]byte` inherited the array rules: the shape behind
// Tekton pipeline's last gocritic-adjacent diff — `fmt.Sprintf("%x",
// hasher.Sum(nil))` — went unreported because a call is not an identifier, and
// a `[]byte` that *was* an identifier got rewritten to
// `hex.EncodeToString(b[:])`. A non-byte element fell through the same
// predicate as an array and reported `[]int` under `%x`.
//
// The whole fixture before this file had one `%x` shape: a `[]byte`
// identifier, the one case the collapsed predicate happened to get right.
//
// `// FINDING` marks a line golangci-lint 2.12.2 reports; the rest are silent
// in both tools. Measured 2026-09-02.

type digest []byte

type box struct {
	sl  []byte
	arr [4]byte
}

func sum() []byte { return nil }

func arr4() [4]byte { return [4]byte{} }

func HexFromCall() string { return fmt.Sprintf("%x", sum()) } // FINDING

func HexSliceIdent(b []byte) string { return fmt.Sprintf("%x", b) } // FINDING

func HexSliceField(x box) string { return fmt.Sprintf("%x", x.sl) } // FINDING

func HexSliceLiteral() string { return fmt.Sprintf("%x", []byte{1, 2}) } // FINDING

func HexArrayIdent(a [4]byte) string { return fmt.Sprintf("%x", a) } // FINDING

// The type assertion upstream makes is on the type itself, not its underlying
// type (`valueType.(*types.Slice)`), so a defined slice type is neither case.
func HexSliceNamed(d digest) string { return fmt.Sprintf("%x", d) }

// Only the array case refuses a non-identifier, and it refuses all three
// spellings.
func HexArrayField(x box) string { return fmt.Sprintf("%x", x.arr) }

func HexArrayLiteral() string { return fmt.Sprintf("%x", [2]byte{1, 2}) }

func HexArrayCall() string { return fmt.Sprintf("%x", arr4()) }

// A non-byte element is neither case.
func HexIntSlice(v []int) string { return fmt.Sprintf("%x", v) }

func HexIntArray(v [4]int) string { return fmt.Sprintf("%x", v) }

// Only lower-case `%x` is a hex-format shape.
func HexCapitalX(b []byte) string { return fmt.Sprintf("%X", b) }

func HexSliceWithV(b []byte) string { return fmt.Sprintf("%v", b) }

func HexOnString(s string) string { return fmt.Sprintf("%x", s) }

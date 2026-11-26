// Package formatter holds the two rules that decide whether printf looks at an
// operand at all: `isFormatter`, and the byte-array/slice special case in
// `matchArgType`.
//
// Every function is marked `// fires` or `// silent`, and the silent ones are
// the point — both rules exist to *suppress* diagnostics, so a port that drops
// them is strictly noisier than upstream and says so on real code (tailscale's
// `net/tstun` and `types/geo`).
package formatter

import "fmt"

type withFormat struct{}

func (withFormat) Format(f fmt.State, verb rune) {}

type plain struct{ N int }

// silent — an `error` argument. `isFormatter` answers yes for **any** interface
// that is not a type parameter, because the dynamic value it holds could
// implement `fmt.Formatter`, and then *no* check applies to the operand: not
// the verb, not the type. tailscale's `t.Fatalf("UnmarshalUint64: err %r, …")`
// is this shape, and `go vet` says nothing about it.
func unknownVerbOnError(err error) string { return fmt.Sprintf("%r", err) }

// silent — the same reasoning for a wrong type.
func wrongTypeOnError(err error) string { return fmt.Sprintf("%d", err) }

// silent — `any` is an interface too.
func unknownVerbOnAny(v any) string { return fmt.Sprintf("%y", v) }

// silent — a concrete type that really does implement `fmt.Formatter`.
func unknownVerbOnFormatter(w withFormat) string { return fmt.Sprintf("%y", w) }

// fires — a `string` is not a formatter, so the unknown verb is reported.
func unknownVerbOnString(s string) string { return fmt.Sprintf("%y", s) }

// fires — nor is a plain struct.
func unknownVerbOnStruct(p plain) string { return fmt.Sprintf("%y", p) }

// silent — `%w` is exempt from the formatter test ("Skip check for the %w
// verb, which requires an error"), so the error-wrapping diagnostics survive.
// Here the call *is* a wrapper, so there is nothing to report.
func wrapOk(err error) error { return fmt.Errorf("ctx: %w", err) }

// fires — and here it is not, which is the diagnostic the exemption protects.
func wrapInSprintf(err error) string { return fmt.Sprintf("%w", err) }

// silent — a byte array prints like a string, same as a byte slice.
// tailscale's `fmt.Sprintf("%s%s%s", disco.Magic, discobs[:], [24]byte{})`.
func byteArrayToS() string { var a [24]byte; return fmt.Sprintf("%s", a) }

func byteArrayLiteralToS() string { return fmt.Sprintf("%s", [24]byte{}) }

// silent — a named byte array is still a byte array.
type key [32]byte

func namedByteArrayToS(k key) string { return fmt.Sprintf("%s", k) }

// silent — and the slice form it mirrors.
func byteSliceToS(b []byte) string { return fmt.Sprintf("%s %q %x", b, b, b) }

// fires — **byte only**. `fmt` prints a rune slice as a list of int32, which is
// exactly what the check is for; accepting it here cost the finding upstream
// makes.
func runeSliceToS(r []rune) string { return fmt.Sprintf("%s", r) }

// fires — an array of anything else recurses into the element, as before.
func intArrayToS() string { var a [3]int; return fmt.Sprintf("%s", a) }

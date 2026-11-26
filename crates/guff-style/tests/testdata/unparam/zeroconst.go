// Package zeroconst is unparam's "always receives" over call sites that spell
// the same constant in different ways.
//
// go/ssa's `NewConst` normalizes a zero value: the constant for `var s string`
// carries `""`, not "no value", because `soleTypeKind` says every type in the
// set agrees on what its zero looks like. `eqlConsts` then compares it equal to
// a written `""`. A zero constant that keeps "no value" instead disagrees with
// the literal, and the parameter goes unreported — authelia's
// `runCryptoPairGenerate` has four call sites writing `""` and one passing a
// `var privateKeyLegacyPath string` straight down.
//
// unparam wants at least four call sites before it will call a value constant,
// so each function below has exactly four.
package zeroconst

// fires — one site passes a zero-valued variable, three write the literal.
func mixedString(sink *[]string, dir, legacyPath string) error {
	if legacyPath != "" {
		*sink = append(*sink, legacyPath)
	}

	*sink = append(*sink, dir)

	return nil
}

func MixedString1(sink *[]string, d string) error {
	var legacyPath string

	return mixedString(sink, d, legacyPath)
}

func MixedString2(sink *[]string, d string) error { return mixedString(sink, d, "") }
func MixedString3(sink *[]string, d string) error { return mixedString(sink, d, "") }
func MixedString4(sink *[]string, d string) error { return mixedString(sink, d, "") }

// fires — the same for an int, whose zero normalizes to `0`.
func mixedInt(sink *[]string, dir string, n int) error {
	if n > 0 {
		*sink = append(*sink, dir)
	}

	return nil
}

func MixedInt1(sink *[]string, d string) error {
	var n int

	return mixedInt(sink, d, n)
}

func MixedInt2(sink *[]string, d string) error { return mixedInt(sink, d, 0) }
func MixedInt3(sink *[]string, d string) error { return mixedInt(sink, d, 0) }
func MixedInt4(sink *[]string, d string) error { return mixedInt(sink, d, 0) }

// fires — and a bool, whose zero normalizes to `false`.
func mixedBool(sink *[]string, dir string, on bool) error {
	if on {
		*sink = append(*sink, dir)
	}

	return nil
}

func MixedBool1(sink *[]string, d string) error {
	var on bool

	return mixedBool(sink, d, on)
}

func MixedBool2(sink *[]string, d string) error { return mixedBool(sink, d, false) }
func MixedBool3(sink *[]string, d string) error { return mixedBool(sink, d, false) }
func MixedBool4(sink *[]string, d string) error { return mixedBool(sink, d, false) }

// fires — a pointer's zero keeps "no value", which is what nil means, and a
// written `nil` is the same constant. This is the control for the branch that
// must *not* normalize.
//
// Every site writes `nil` on purpose. Four sites passing `var p *int` instead
// is a **known divergence**, still open: `eqlConsts` compares `types.Type` by
// pointer identity, go/types does not intern `*int`, so each `var p *int`
// declares a different type object and upstream answers "not the same
// constant". guff's arena interns structural types, so it answers "the same"
// and reports. Nothing here can model object identity, so the shape is left
// out rather than pinned wrong.
func nilPointer(sink *[]string, dir string, p *int) error {
	if p != nil {
		*sink = append(*sink, dir)
	}

	return nil
}

func NilPointer1(sink *[]string, d string) error { return nilPointer(sink, d, nil) }
func NilPointer2(sink *[]string, d string) error { return nilPointer(sink, d, nil) }
func NilPointer3(sink *[]string, d string) error { return nilPointer(sink, d, nil) }
func NilPointer4(sink *[]string, d string) error { return nilPointer(sink, d, nil) }

// silent — two different constants.
func mixedDiffers(sink *[]string, dir, tag string) error {
	*sink = append(*sink, dir+tag)

	return nil
}

func MixedDiffers1(sink *[]string, d string) error {
	var tag string

	return mixedDiffers(sink, d, tag)
}

func MixedDiffers2(sink *[]string, d string) error { return mixedDiffers(sink, d, "") }
func MixedDiffers3(sink *[]string, d string) error { return mixedDiffers(sink, d, "x") }
func MixedDiffers4(sink *[]string, d string) error { return mixedDiffers(sink, d, "") }

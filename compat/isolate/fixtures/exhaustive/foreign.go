package p

import "example.com/enumdep"

// An enum declared in an imported package. Upstream reads its members from an
// object fact, exported when it analysed that package; guff never analyses a
// package it only imports, so the members come from the package scope instead.

func ForeignSwitch(k enumdep.Kind) string {
	switch k {
	case enumdep.KindZ:
		return "z"
	}
	return ""
}

// Same-valued members report one representative, and the unexported one is
// never demanded of another package.
func ForeignSameValue(f enumdep.Flag) string {
	switch f {
	case enumdep.FlagOne:
		return "one"
	}
	return ""
}

// Every member handled: silent.
func ForeignComplete(k enumdep.Kind) string {
	switch k {
	case enumdep.KindZ:
		return "z"
	case enumdep.KindA:
		return "a"
	case enumdep.KindM:
		return "m"
	}
	return ""
}

// Only the exported member can be missing.
func ForeignMostlyUnexported(m enumdep.Mostly) string {
	switch m {
	case enumdep.MostlyOpen:
		return "open"
	}
	return ""
}

// A `default` clause does not satisfy exhaustiveness by default.
func ForeignDefault(k enumdep.Kind) string {
	switch k {
	case enumdep.KindZ:
		return "z"
	default:
		return "?"
	}
}

// An **alias** to the enum is a *types.Alias, which upstream's fromType does
// not match: silent, however many members the aliased type has.
func ForeignAlias(k enumdep.KindAlias) string {
	switch k {
	case enumdep.KindZ:
		return "z"
	}
	return ""
}

// A *defined* type over the enum has no members of its own: silent.
type MyKind enumdep.Kind

func ForeignDefined(k MyKind) string {
	switch k {
	case MyKind(enumdep.KindZ):
		return "z"
	}
	return ""
}

// The tag is a conversion to the foreign enum.
func ForeignConversion(s string) string {
	switch enumdep.Kind(s) {
	case enumdep.KindZ:
		return "z"
	}
	return ""
}

// Not an enum at all.
func ForeignNotEnum(s enumdep.Struct) int {
	switch s.N {
	case 1:
		return 1
	}
	return 0
}

// A map literal keyed by the foreign enum (reached under `check: [map]`).
var ForeignMap = map[enumdep.Kind]int{
	enumdep.KindZ: 1,
}

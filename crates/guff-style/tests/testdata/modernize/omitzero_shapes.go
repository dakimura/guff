//go:build go1.24

// Package omitzeroshapes holds the tag spellings that decide *where* omitzero's
// two suggested fixes are cut from, which the plain fixture cannot show: it has
// only raw-string tags with a json name, so every offset it produces is the
// same trivial one.
//
// Upstream's fixes are ranges inside the string literal, mapped through
// `astutil.RangeInStringLiteral` — so an escaped tag shifts them — and the
// removal alternative widens to the json tag, or to the whole literal, when
// `json` carries nothing but `,omitempty`.
package omitzeroshapes

type Escaped struct {
	// An interpreted string literal: `\"` is two bytes of source for one of
	// value, so the cooked offset of `,omitempty` is not its source offset.
	Value NestedValue "json:\"value,omitempty\""
}

type JSONOnly struct {
	// `json` is the only tag and holds nothing but the option: upstream removes
	// the entire literal, backquotes included.
	Ref NestedSecret `json:",omitempty"`
}

type JSONAmongOthers struct {
	// Same, but another key follows, so only the json tag goes.
	Other NestedValue `json:",omitempty" xml:"other"`
}

type NestedValue struct{ Raw string }

type NestedSecret struct{ Name string }

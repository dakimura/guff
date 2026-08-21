package main

// SA5008's JSON half is a port of encoding/json/v2's tag parser
// (honnef.co/go/tools/staticcheck/sa5008/jsonv2.go), so the option grammar is
// **v2's**: options v1 never had are accepted, and the near-misses v2 rejects
// are named rather than lumped in with unknown options.
//
// compat/golden/cases/staticcheck-sa runs golangci-lint 2.12.2 over this same
// file, so every line below is checked against upstream.
//
// Doc comments here start with the type name on purpose: the golden case runs
// the ST checks too, and ST1021 would otherwise bury the SA5008 findings.

type T struct {
	X int `xml:"a,unknown"`
}

// BadEscape holds the one shape that is a parse error: the *value* will not
// unquote. Malformed tag *structure* is silent — see ok.go.
type BadEscape struct {
	E string `json:"\q"`
}

// TrailingComma has a trailing comma with no option after it. This is the
// thanos `pkg/api/api.go` shape that guff used to miss entirely.
type TrailingComma struct {
	Name string `json:"name"`
	Emb  int    `json:","`
}

// Options exercises the option grammar; a mutant of a real option is named,
// not reported as unknown, and `format` must come last.
type Options struct {
	A int `json:"a,omitEmpty"`
	B int `json:"b,omit_empty"`
	C int `json:"c,bogus"`
	D int `json:"d,omitempty,omitempty"`
	E int `json:"e,inline,unknown"`
	F int `json:"f,case"`
	G int `json:"g,case:loud"`
	H int `json:"h,format:RFC3339,omitempty"`
	I int `json:"i,format"`
}

// StringOption shows that `string` is only for numeric fields and pointers to
// them (bool and string are tolerated because v1 supported them by accident).
type StringOption struct {
	Bytes []byte          `json:"bytes,string"`
	Struc struct{ X int } `json:"struc,string"`
}

// Unexported covers the two field-level cases.
type Unexported struct {
	lower int `json:"lower"`
	Dash  int `json:"-,omitempty"`
}

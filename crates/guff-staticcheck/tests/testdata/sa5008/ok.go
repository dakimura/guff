package main

type T struct {
	X int `json:"name"`
}

// Unparseable shows that malformed tag *structure* ends the scan without a
// diagnostic: upstream's parseStructTag `break`s rather than erroring, so none
// of these is a finding. guff used to report `unparseable struct tag:
// malformed struct tag` on all four.
type Unparseable struct {
	A string `notatag`
	B string `json:"b" trailing`
	C string `json`
	D string `json:"e`
}

// Escapes holds values that unquote cleanly.
type Escapes struct {
	A string `json:"a\tb"`
	B string `json:"\x41"`
}

// Options lists every option v2 accepts.
type Options struct {
	A int             `json:"a,omitempty"`
	B int             `json:"b,omitzero"`
	C int             `json:"c,case:ignore"`
	D int             `json:"d,case:strict"`
	E int             `json:"e,format:RFC3339"`
	F struct{ X int } `json:"f,inline"`
	G int             `json:"-"`
}

// StringOption shows where `string` applies: numbers and pointers to numbers,
// plus bool and string for v1 compatibility.
type StringOption struct {
	Num  int     `json:"num,string"`
	Ptr  *int    `json:"ptr,string"`
	Str  string  `json:"str,string"`
	Flt  float64 `json:"flt,string"`
	Bool bool    `json:"bool,string"`
}

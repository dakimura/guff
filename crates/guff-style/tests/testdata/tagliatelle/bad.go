package tagliatelle

type Foo struct {
	ID     string `json:"ID"`
	UserID string `json:"UserID"`
	Name   string `json:"name"`
	Value  string `json:"value,omitempty"`
	Bar    Bar    `json:"bar"`
}

type Bar struct {
	Name  string `json:"-"`
	Value string `json:"value"`
	Item  *Bir   `json:"CommonServiceItem,omitempty"`
}

type Bir struct {
	Name  string `json:"-"`
	Value string `json:"value"`
}

type Bur struct {
	Name  string
	Value string `yaml:"Value"`
	Also  string `json:"also,omitempty"`
}

// A digit belongs to the word it follows: `Name2` is one word, `Foo2Bar` is
// `Foo2` + `Bar`, `H2C` is `H2` + `C`. `header` is checked with no rule in the
// config because golangci-lint's wrapper defaults it to `header`, which is how
// fiber's `header:"Name2"` reached this at all.
type Digits struct {
	A string `json:"Name2"`
	B string `json:"Foo2Bar"`
	C string `json:"H2C"`
	D string `json:"A1B2"`
	E string `json:"HTTP2Server"`
	F string `header:"Name2"`
	G string `header:"Foo2Bar"`
	H string `header:"H2C"`
}

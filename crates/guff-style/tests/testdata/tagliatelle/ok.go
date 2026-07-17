package tagliatelle_ok

type Foo struct {
	ID     string `json:"id"`
	UserID string `json:"userId"`
	Name   string `json:"name"`
	Value  string `json:"value,omitempty"`
	Bar    Bar    `json:"bar"`
}

type Bar struct {
	Name  string `json:"-"`
	Value string `json:"value"`
	Item  *Bir   `json:"commonServiceItem,omitempty"`
}

type Bir struct {
	Name  string `json:"-"`
	Value string `json:"value"`
}

type Bur struct {
	Name  string
	Value string `yaml:"value"`
	Also  string `json:"also,omitempty"`
	Hdr   string `header:"X-Request-Id"`
}

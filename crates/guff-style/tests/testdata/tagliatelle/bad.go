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

package p

// tagliatelle checks one tag per configured convention, and the message names
// both the tag and the case it wanted. One struct field with one tag reaches
// one of them.

type Bad struct {
	UserId  int    `json:"UserId"`
	UserAge int    `yaml:"UserAge"`
	Name    string `xml:"Name"`
	Extra   string `mapstructure:"Extra"`
}

// Digits belong to the word they follow and never start one of their own:
// `Name2` is a single word, `Foo2Bar` is `Foo2` + `Bar`, `H2C` is `H2` + `C`,
// and `A2b` is one word. Splitting at the digit put a separator in front of
// every one of them — fiber's `header:"Name2"` was told to want `Name-2`.
// `header` needs no rule here: golangci-lint's wrapper defaults it to
// `{json: camel, yaml: camel, header: header}` before the config is merged in.
//
// The eight strings run through five conventions; every one was measured
// against golangci-lint 2.12.2 one at a time.

type Digits struct {
	A string `json:"Name2"       yaml:"Foo2Bar"    xml:"A1B2"        mapstructure:"H2C"      header:"Name2"`
	B string `json:"HTTP2Server" yaml:"IPv4"       xml:"ABC2"        mapstructure:"A2b"      header:"Foo2Bar"`
	C string `json:"A2b"         yaml:"Name22"     xml:"HTTP2Server" mapstructure:"IPv4"     header:"A1B2"`
	D string `json:"X2"          yaml:"H2C"        xml:"Name2"       mapstructure:"Foo2Bar"  header:"H2C"`
	E string `json:"2Name"       yaml:"ABC2"       xml:"Foo2"        mapstructure:"Name2"    header:"HTTP2Server"`
	F string `json:"IPv4"        yaml:"A1B2"       xml:"A2b"         mapstructure:"X2"       header:"IPv4"`
}

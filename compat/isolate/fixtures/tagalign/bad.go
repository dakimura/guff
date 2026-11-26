package p

type Example struct {
	Foo int `json:"foo"        yaml:"foo"`
	Bar int `yaml:"bar"  json:"bar"`
	Baz int `json:"baz" xml:"baz" yaml:"baz"`
}

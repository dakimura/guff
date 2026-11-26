package tagalign

type Example struct {
	Foo int `json:"foo" yaml:"foo"`
	Bar int `json:"bar" yaml:"bar"`
}

type Single struct {
	Only int `json:"only"`
}

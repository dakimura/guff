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

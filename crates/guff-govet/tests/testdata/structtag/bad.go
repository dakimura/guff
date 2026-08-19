package bad

type T struct {
	x int `json:"name"`
}

// Tag options are not part of the encoding name: `json:"a,omitempty"` and
// `json:"a"` name the same field and collide.
type OptionsAreNotNames struct {
	A  int `json:"a,omitempty"`
	A2 int `json:"a"`
}

// XML attributes get a namespace of their own, so an attribute and an element
// may share a name...
type AttrNamespace struct {
	Kind  string `xml:"kind,attr"`
	Kind2 string `xml:"kind"`
}

// ...but two attributes of the same name still collide.
type AttrDuplicate struct {
	Kind  string `xml:"kind,attr"`
	Kind2 string `xml:"kind,attr"`
}

// XMLName names the element of the struct being checked, so it cannot collide
// with the element names of that struct's own fields — the repeat here is not
// a finding (gitea services/migrations/codebase.go). vet keys the exemption on
// the field name alone, so the field does not have to be an `xml.Name`.
type XMLNameIsExempt struct {
	XMLName string   `xml:"ticketing-milestone"`
	Items   []string `xml:"ticketing-milestone"`
}

// `reflect.StructTag.Get` returns the options too, so a tag that is only
// options still counts as tagged for the unexported-field check.
type OptionsOnlyOnUnexported struct {
	hidden int `json:",omitempty"`
}

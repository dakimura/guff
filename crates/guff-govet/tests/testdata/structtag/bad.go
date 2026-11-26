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

// The tag guff cannot parse is rendered with `%#q`, which prefers a backquoted
// string and falls back to `strconv.Quote`. Both arms are here, because almost
// every tag takes the first one and nothing in the suite used to take either.
//
// These also pin the *value*: a tag written as an interpreted string literal
// has to be unquoted with Go's rules, not by dropping backslashes.
type InvalidTagRendering struct {
	// Backquoted: plain ASCII, a space, a tab (the one control character a
	// backquoted string may hold), a double quote (needs no escape there), and
	// a multibyte rune (upstream assumes every one of them printable).
	Plain    string `bson:_id`
	Space    string `bson: _id`
	Tab      string "bson:\t_id"
	Quote    string "bson:\"_id"
	NonASCII string `bson:_idé`
	// A backquoted literal keeps its backslash, so the value really is
	// `bson:\_id` and it still backquotes.
	Backslash string `bson:\_id`

	// Quoted: a backquote cannot appear inside one, a control character and
	// U+007F are not printable, and a BOM is invisible.
	Backquote string "bson:`_id"
	Newline   string "bson:\n_id"
	Del       string "bson:\x7f_id"
	Bom       string "bson:\ufeff_id"
}

// The repeats message renders the encoding name with plain `%q` — no backquote
// arm — and needs the same unquoting underneath it.
type RepeatedNameRendering struct {
	Apostrophe  string `json:"dup'x"`
	Apostrophe2 string `json:"dup'x"`
	Backquote   string "json:\"dup`y\""
	Backquote2  string "json:\"dup`y\""
	Del         string "json:\"dup\\x7fz\""
	Del2        string "json:\"dup\\x7fz\""
	Tab         string "json:\"dup\\tw\""
	Tab2        string "json:\"dup\\tw\""
}

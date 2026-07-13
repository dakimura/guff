package url
type URL struct { RawQuery string }
type Values map[string][]string
func (u *URL) Query() Values { return Values{} }
func (v Values) Set(key, value string) {}
func (v Values) Encode() string { return "" }

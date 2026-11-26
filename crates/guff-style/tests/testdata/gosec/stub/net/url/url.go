package url

type URL struct {
	Scheme   string
	Host     string
	Path     string
	RawQuery string
}

func (u *URL) String() string  { return "" }
func (u *URL) Query() Values   { return nil }

type Values map[string][]string

func (v Values) Get(key string) string { return "" }

func Parse(rawURL string) (*URL, error) { return nil, nil }
func QueryEscape(s string) string       { return s }
func PathEscape(s string) string        { return s }

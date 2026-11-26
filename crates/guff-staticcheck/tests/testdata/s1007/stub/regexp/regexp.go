package regexp

type Regexp struct{}
type regexpError string
func (e regexpError) Error() string { return string(e) }
func Compile(s string) (*Regexp, error) { return &Regexp{}, regexpError("") }

package regexp

type Regexp struct{}

func Compile(expr string) (*Regexp, error) { return &Regexp{}, nil }
func CompilePOSIX(expr string) (*Regexp, error) { return &Regexp{}, nil }
func MustCompile(str string) *Regexp { return &Regexp{} }
func MustCompilePOSIX(str string) *Regexp { return &Regexp{} }

package regexp

type Regexp struct{}

func MustCompile(str string) *Regexp { return &Regexp{} }

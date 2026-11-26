package regexp

type Regexp struct{}

func MustCompile(s string) *Regexp {
	var r Regexp
	return &r
}

func Compile(s string) (r *Regexp, err error) {
	var out Regexp
	r = &out
	return
}

func Match(pattern string, b []byte) (matched bool, err error) {
	return
}

func MatchReader(pattern string, r any) (matched bool, err error) {
	return
}

func MatchString(pattern string, s string) (matched bool, err error) {
	return
}

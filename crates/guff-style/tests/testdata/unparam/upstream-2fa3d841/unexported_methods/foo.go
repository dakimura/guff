package foo

type fooType struct {
	version string
}

func NewFooType(version string) *fooType {
	return &fooType{version: version}
}

func (f *fooType) Run() string {
	if f.version != "" {
		clean := func(s string, unusedParam int) string {
			return apply(s)
		}
		return clean(f.version, 1)
	}
	return ""
}

func (f *fooType) oneUnused(a int) string {
	return f.version
}

func apply(version string) string {
	return "v" + version
}

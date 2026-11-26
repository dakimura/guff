package regexp

func MatchString(pattern string, s string) (bool, error) { return false, nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }

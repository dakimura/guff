package template

type Template struct{}

func New(name string) *Template { return &Template{} }

func (t *Template) Parse(text string) (*Template, error) { return t, nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }

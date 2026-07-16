package testing

type T struct{}

type TB interface {
	Name() string
}

func (t *T) Name() string { return "" }

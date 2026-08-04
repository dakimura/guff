package p

func NewT() *T { return &T{} }

type T struct{}

func helper() {}

func (T) Exported() {}

func (T) unexported() {}

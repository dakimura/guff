package funcorder

// NewOther is a constructor declared before its struct type.
func NewOther() *Other {
	return &Other{}
}

type Other struct {
	Name string
}

func (o Other) lenName() int {
	return len(o.Name)
}

func (o Other) GetName() string {
	return o.Name
}

type Third struct{}

func (t Third) Do() string { return "" }

// NewThird is a constructor declared after a struct method.
func NewThird() Third { return Third{} }

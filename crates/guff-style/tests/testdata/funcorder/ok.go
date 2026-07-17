package funcorder

type Good struct {
	Name string
}

func NewGood() Good {
	return Good{Name: "x"}
}

func (g Good) GetName() string {
	return g.Name
}

func (g Good) lenName() int {
	return len(g.Name)
}

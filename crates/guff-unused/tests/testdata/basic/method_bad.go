package bad

type usedType struct{}

func (usedType) used() {}

func (usedType) unusedMethod() {}

func Run() {
	var t usedType
	t.used()
}

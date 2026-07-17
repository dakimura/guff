package gochecksumtype

//sumtype:decl
type SumType interface{ isSumType() }

type One struct{}

func (One) isSumType() {}

type Two struct{}

func (Two) isSumType() {}

func settingsDefaultNotExhaustive() {
	var sum SumType = One{}
	switch sum.(type) {
	case One:
	default:
		_ = sum
	}
}

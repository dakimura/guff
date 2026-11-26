package pkg

type BasicOuter struct{ BasicInner }
type BasicInner struct{ F1 int }

func fn() {
	var basic BasicOuter
	_ = basic.BasicInner.F1
}

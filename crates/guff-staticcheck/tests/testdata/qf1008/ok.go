package pkg

type BasicOuter struct{ BasicInner }
type BasicInner struct{ F1 int }

type NotEmbedded struct {
	Inner BasicInner
}

func fn() {
	var basic BasicOuter
	_ = basic.F1

	var n NotEmbedded
	_ = n.Inner.F1
}

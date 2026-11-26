package pkg

type FunctionCallOuter struct{ FunctionCallInner }
type FunctionCallInner struct {
	F8 func() FunctionCallContinuedOuter
}
type FunctionCallContinuedOuter struct{ FunctionCallContinuedInner }
type FunctionCallContinuedInner struct{ F9 int }

func fnCall() {
	var call FunctionCallOuter
	_ = call.FunctionCallInner.F8().FunctionCallContinuedInner.F9
	_ = call.F8().F9
}

type MethodOuter struct{ MethodInner }
type MethodInner struct{}

func (MethodInner) M() int { return 0 }

func fnMethodCall() {
	var o MethodOuter
	_ = o.MethodInner.M()
}

package diff

var debug debugger

type debugger struct{}

func (debugger) Begin(_, _ int, f EqualFunc, _, _ *EditScript) EqualFunc {
	panic("stub")
}
func (debugger) Update() {
	panic("stub")
}
func (debugger) Finish() {
	panic("stub")
}

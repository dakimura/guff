package cmp

type defaultReporter struct {
	root *valueNode
	curr *valueNode
}

func (r *defaultReporter) PushStep(ps PathStep) {
	panic("stub")
}
func (r *defaultReporter) Report(rs Result) {
	panic("stub")
}
func (r *defaultReporter) PopStep() {
	panic("stub")
}
func (r *defaultReporter) String() string {
	panic("stub")
}
func assert(ok bool) {
	panic("stub")
}

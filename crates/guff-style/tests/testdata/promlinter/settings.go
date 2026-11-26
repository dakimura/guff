package promlinter

type CounterOpts struct {
	Name string
	Help string
}

type CounterVec struct{}

func NewCounterVec(opts CounterOpts, labelNames []string) *CounterVec { return nil }

func settingsCase() {
	// Would fail Counter check unless disabled.
	_ = NewCounterVec(CounterOpts{
		Name: "test_metric_name",
		Help: "test help text",
	}, []string{})
}

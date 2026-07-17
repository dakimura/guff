package promlinter

// Minimal stubs so the file typechecks without real prometheus.
type CounterOpts struct {
	Namespace string
	Subsystem string
	Name      string
	Help      string
}

type GaugeOpts struct {
	Name string
	Help string
}

type CounterVec struct{}
type Gauge struct{}
type Counter struct{}

func NewCounterVec(opts CounterOpts, labelNames []string) *CounterVec { return nil }
func NewCounter(opts CounterOpts) Counter                             { return Counter{} }
func NewGauge(opts GaugeOpts) Gauge                                   { return Gauge{} }
func NewCounterFunc(opts CounterOpts, function func() float64) Counter {
	return Counter{}
}

func bad() {
	// counter without _total
	_ = NewCounterVec(CounterOpts{
		Name: "test_metric_name",
		Help: "test help text",
	}, []string{})

	// missing Help field
	_ = NewCounterVec(CounterOpts{
		Name: "test_metric_total",
	}, []string{})

	// NewCounterFunc without _total
	_ = NewCounterFunc(CounterOpts{
		Name: "foo",
		Help: "bar",
	}, func() float64 { return 1 })

	// camelCase name
	_ = NewCounter(CounterOpts{
		Name: "httpRequests_total",
		Help: "ok",
	})

	// gauge with _total
	_ = NewGauge(GaugeOpts{
		Name: "widgets_total",
		Help: "should not use total",
	})

	// non-base unit
	_ = NewGauge(GaugeOpts{
		Name: "job_duration_minutes",
		Help: "duration",
	})
}

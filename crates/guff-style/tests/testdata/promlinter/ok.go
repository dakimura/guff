package promlinter

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

func ok() {
	_ = NewCounterVec(CounterOpts{
		Name: "test_metric_total",
		Help: "",
	}, []string{})

	_ = NewCounter(CounterOpts{
		Namespace: "app",
		Subsystem: "api",
		Name:      "requests_total",
		Help:      "number of requests",
	})

	_ = NewGauge(GaugeOpts{
		Name: "queue_depth",
		Help: "current queue depth",
	})

	// empty Name → skipped (stub metric)
	_ = NewCounter(CounterOpts{})
}

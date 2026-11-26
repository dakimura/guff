// Stub of the prometheus client, at the real import path: promlinter matches
// `prometheus.New*` calls by package path and constructor name, then reads the
// Opts literal's Name/Help/Namespace/Subsystem fields.
package prometheus

type Counter interface{ Inc() }
type Gauge interface{ Set(float64) }
type Histogram interface{ Observe(float64) }
type Summary interface{ Observe(float64) }

type CounterOpts struct {
	Namespace string
	Subsystem string
	Name      string
	Help      string
}

type GaugeOpts CounterOpts
type HistogramOpts struct {
	Namespace string
	Subsystem string
	Name      string
	Help      string
	Buckets   []float64
}
type SummaryOpts struct {
	Namespace string
	Subsystem string
	Name      string
	Help      string
}

func NewCounter(opts CounterOpts) Counter          { return nil }
func NewGauge(opts GaugeOpts) Gauge                { return nil }
func NewHistogram(opts HistogramOpts) Histogram    { return nil }
func NewSummary(opts SummaryOpts) Summary          { return nil }

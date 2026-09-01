package typeinname

// Minimal stubs so the file typechecks without real prometheus.
type GaugeOpts struct {
	Namespace string
	Subsystem string
	Name      string
	Help      string
}

type Gauge struct{}

func NewGauge(opts GaugeOpts) Gauge { return Gauge{} }

// MetricTypeInName looks for *every* metric type name, not the metric's own.
// golangci-lint builds promlinter v0.3.0 against client_golang v1.12.1, whose
// rule ranges over all of dto.MetricType_name and skips only UNTYPED.
func typeInName() {
	// syncthing lib/model/metrics.go: a gauge named after another type.
	_ = NewGauge(GaugeOpts{Name: "folder_summary", Help: "n"})
	// A gauge named after its own type.
	_ = NewGauge(GaugeOpts{Name: "queue_gauge", Help: "n"})
	// The type name in the middle rather than at the end.
	_ = NewGauge(GaugeOpts{Name: "queue_histogram_depth", Help: "n"})
	// Two type names: one finding each.
	_ = NewGauge(GaugeOpts{Name: "queue_counter_gauge", Help: "n"})

	// Silent: 'untyped' is the one name the rule skips.
	_ = NewGauge(GaugeOpts{Name: "queue_untyped", Help: "n"})
	// Silent: not delimited by underscores.
	_ = NewGauge(GaugeOpts{Name: "queuegauge", Help: "n"})
	// Silent: no type name at all.
	_ = NewGauge(GaugeOpts{Name: "queue_depth", Help: "n"})
}

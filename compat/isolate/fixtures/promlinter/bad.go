package p

import "github.com/prometheus/client_golang/prometheus"

func Bad() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: "requests", Help: "n"})
}

// promlinter carries a rule per naming convention, and each names the metric
// and the rule it broke — separate sentences, not one repeated.
func MissingHelp() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queue_depth"})
}

func CounterWithoutTotal() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: "requests_made", Help: "n"})
}

func NonBaseUnit() {
	_ = prometheus.NewHistogram(prometheus.HistogramOpts{
		Name: "latency_milliseconds",
		Help: "n",
	})
}

func CamelCaseName() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queueDepth", Help: "n"})
}

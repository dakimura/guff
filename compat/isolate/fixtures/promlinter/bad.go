package p

import "github.com/prometheus/client_golang/prometheus"

func Bad() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: "requests", Help: "n"})
}

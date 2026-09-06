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

// MetricTypeInName looks for *every* metric type name, not the metric's own:
// golangci-lint builds promlinter v0.3.0 against client_golang v1.12.1, whose
// rule ranges over all of dto.MetricType_name and skips only UNTYPED. (A newer
// client_golang checkout says the opposite — it returns early unless the name
// carries the metric's own type.) syncthing's is the first shape below: a
// gauge called `..._folder_summary`.
func GaugeNamedAfterAnotherType() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "folder_summary", Help: "n"})
}

func GaugeNamedAfterItsOwnType() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queue_gauge", Help: "n"})
}

// The type name in the middle rather than at the end.
func TypeNameInTheMiddle() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queue_histogram_depth", Help: "n"})
}

// Two type names in one metric name: one finding each.
func TwoTypeNames() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queue_counter_gauge", Help: "n"})
}

// 'untyped' is the one name the rule skips.
func UntypedInName() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queue_untyped", Help: "n"})
}

// Not delimited by underscores, so not a match.
func TypeNameNotDelimited() {
	_ = prometheus.NewGauge(prometheus.GaugeOpts{Name: "queuegauge", Help: "n"})
}

// promlinter resolves `Namespace` / `Subsystem` / `Name` through
// `ast.Ident.Obj.Decl` when they are given as names rather than literals, and
// **drops the whole metric** when it cannot (`parseOpts` returns nil on the
// first field it fails to parse). cert-manager writes `Namespace: namespace`
// on every one of its metrics, so guff — which read string literals only —
// reported none of the eight findings golangci-lint does.
const namespace = "app"

const metricName = "requests_from_const"

var varName = "requests_from_var"

func NamespaceFromConst() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{
		Namespace: namespace,
		Name:      "requests_ns_const",
		Help:      "n",
	})
}

func NameFromConst() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: metricName, Help: "n"})
}

func NameFromVar() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: varName, Help: "n"})
}

// `ast.Object.Decl` is an `*ast.AssignStmt` here, which upstream's `parseValue`
// leaves unresolved on purpose (its own TODO) — so the metric is dropped and
// neither tool says anything about `requests_from_short_var`.
func NameFromShortVar() {
	shortName := "requests_from_short_var"
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: shortName, Help: "n"})
}

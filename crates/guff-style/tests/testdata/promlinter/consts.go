// Package promlinter_consts covers the half of promlinter's `parseValue` that
// is not a string literal: a `Namespace` / `Subsystem` / `Name` given as a
// *name*, resolved through `ast.Ident.Obj.Decl` to its `ValueSpec`.
//
// The reach matters as much as the resolution. Upstream leaves an
// `*ast.AssignStmt` unresolved (its own TODO), and a field it cannot parse
// makes `parseOpts` return nil — so the metric is dropped whole rather than
// reported under a half-built name. cert-manager writes `Namespace: namespace`
// on all five of its metrics; guff read literals only and reported none of the
// eight findings golangci-lint does.
package promlinter_consts

import "github.com/prometheus/client_golang/prometheus"

const namespace = "app"

const metricName = "requests_from_const"

var varName = "requests_from_var"

// want: app_requests_ns_const
func NamespaceFromConst() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{
		Namespace: namespace,
		Name:      "requests_ns_const",
		Help:      "n",
	})
}

// want: requests_from_const
func NameFromConst() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: metricName, Help: "n"})
}

// want: requests_from_var
func NameFromVar() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: varName, Help: "n"})
}

// silent: the decl is an `*ast.AssignStmt`, which upstream does not resolve.
func NameFromShortVar() {
	shortName := "requests_from_short_var"
	_ = prometheus.NewCounter(prometheus.CounterOpts{Name: shortName, Help: "n"})
}

// silent: both parts are literals and the name is already well formed.
func LiteralAndFine() {
	_ = prometheus.NewCounter(prometheus.CounterOpts{
		Namespace: "app",
		Name:      "handled_total",
		Help:      "n",
	})
}

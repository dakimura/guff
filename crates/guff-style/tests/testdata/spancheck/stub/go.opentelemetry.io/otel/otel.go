// Stub of the go.opentelemetry.io/otel entry point. `otel.Tracer(name)` is how
// the fixtures obtain a tracer; everything spancheck cares about is the `trace`
// package's types.
package otel

import "go.opentelemetry.io/otel/trace"

func Tracer(name string, opts ...interface{}) trace.Tracer { return nil }

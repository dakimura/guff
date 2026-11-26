// Stub of go.opentelemetry.io/otel/trace, at the real import path so upstream
// spancheck recognises it: the check matches on the package path of the type
// returned by `Start`, not on anything the implementation does.
package trace

import "context"

type Span interface {
	End(options ...SpanEndOption)
	SetStatus(code Code, description string)
	RecordError(err error, options ...EventOption)
}

type Code uint32

type SpanEndOption interface{}
type EventOption interface{}
type SpanStartOption interface{}

type Tracer interface {
	Start(ctx context.Context, spanName string, opts ...SpanStartOption) (context.Context, Span)
}

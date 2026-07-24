package trace

type Context interface{}

type Span interface {
	End()
	SetStatus(code int, msg string)
	RecordError(err error)
}

type Tracer interface {
	Start(ctx Context, name string, opts ...SpanStartOption) (Context, Span)
}

type SpanStartOption struct{}

func Named(name string) Tracer { return tracerImpl{} }

type tracerImpl struct{}

func (tracerImpl) Start(ctx Context, name string, _ ...SpanStartOption) (Context, Span) {
	return ctx, spanImpl{}
}

type spanImpl struct{}

func (spanImpl) End()                           {}
func (spanImpl) SetStatus(code int, msg string)  {}
func (spanImpl) RecordError(err error)          {}

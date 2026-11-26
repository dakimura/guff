package zerolog

type Event struct{}

type Logger struct{}

func New(w any) Logger {
	return Logger{}
}

func (l Logger) Info() *Event {
	return &Event{}
}

func (l Logger) Error() *Event {
	return &Event{}
}

func (e *Event) Str(key, val string) *Event {
	return e
}

// The chain is what zerologlint follows, so the stub needs more than one link
// on it — a fixture that can only call `Str` can only build one shape.
func (e *Event) Int(key string, val int) *Event {
	return e
}

func (e *Event) Bool(key string, val bool) *Event {
	return e
}

func (e *Event) Msg(msg string) {}

func (e *Event) Msgf(format string, v ...any) {}

func (e *Event) Send() {}

package p

type MyError struct{}

func (MyError) Error() string { return "err" }

func (MyError) Unwrap() int { return 0 }

// NonErrorUnwrap must not trip stdmethods — receiver does not implement error
// (x/tools only checks Is/As/Unwrap on error types).
type NonError struct{}

func (NonError) Unwrap() string { return "" }

type Unwrapper interface {
	Unwrap() Backend
}

type Backend interface{}


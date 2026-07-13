package p

type MyError struct{}

func (MyError) Error() string { return "err" }

func (MyError) Unwrap() int { return 0 }

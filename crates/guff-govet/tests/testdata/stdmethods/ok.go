package p

type MyError struct{}

func (MyError) Error() string { return "err" }

func (MyError) Unwrap() error { return nil }

type Key struct{}

func (Key) MarshalJSON() ([]byte, error) { return nil, nil }
func (Key) UnmarshalJSON(data []byte) error { return nil }

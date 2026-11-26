package os

type PathError struct {
	Op   string
	Path string
	Err  error
}

func (e *PathError) Error() string { return "" }

type LinkError struct {
	Op  string
	Old string
	New string
	Err error
}

func (e *LinkError) Error() string { return "" }

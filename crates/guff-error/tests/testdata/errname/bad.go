package errname

type BadErrorType struct{}

func (BadErrorType) Error() string { return "bad" }

var EndOfFileError error

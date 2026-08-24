package p

import "errors"

// err113 has two messages with two different positions: the definition check
// reports the CallExpr, the comparison check reports the BinaryExpr and carries
// a suggested fix.

// "do not define dynamic errors, use wrapped static errors instead: …"
func Define() error {
	return errors.New("dynamic")
}

var ErrSentinel = errors.New("sentinel")

// "do not compare errors directly …, use … instead" — both directions of the
// comparison, since the message renders the operator it saw.
func CompareEqual(err error) bool {
	return err == ErrSentinel
}

func CompareNotEqual(err error) bool {
	return err != ErrSentinel
}

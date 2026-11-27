// Shapes of the go1.27 printf rules beyond upstream's w/ and issue62595/:
// each branch of the `%w`-of-a-pointer check and each operand kind `%d`
// stops accepting.
package shapes

import (
	"fmt"
	"io/fs"
	"syscall"
	"unsafe"
)

type CustomError struct{ Msg string }

func (e CustomError) Error() string { return e.Msg }

// PE's underlying type is *CustomError: upstream looks through it.
type PE *CustomError

type PtrOnly struct{}

func (*PtrOnly) Error() string { return "" }

func Wrap(pe PE, ep *error, errno syscall.Errno) {
	_ = fmt.Errorf("%w", pe)                 // named pointer type
	_ = fmt.Errorf("%w", ep)                 // pointer to the error interface
	_ = fmt.Errorf("%w", &errno)             // another package: qualified by name
	_ = fmt.Errorf("%w", &fs.PathError{})    // *fs.PathError: the value has no Error method
	_ = fmt.Errorf("%w", &PtrOnly{})         // same, local
	_ = fmt.Errorf("%w", errno)              // not a pointer
	_ = fmt.Errorf("%[1]w %[1]w", &errno)    // explicit index: the first failure ends the call
	_ = fmt.Errorf("%w and %w", ep, pe)      // likewise: only the first operand is reported
	_ = fmt.Errorf("%v", ep)                 // other verbs are untouched
}

func Generic[T error](p *T) error {
	return fmt.Errorf("%w", p) // *T where T is constrained by error
}

type S struct{ X int }

func Ints(i int, m map[string]int, s []int, ps *S, f func(), up uintptr) {
	_ = fmt.Sprintf("%d", &i)
	_ = fmt.Sprintf("%d", m)
	_ = fmt.Sprintf("%d", s)
	_ = fmt.Sprintf("%d", ps)
	_ = fmt.Sprintf("%d", &s)
	_ = fmt.Sprintf("%d", up)
	_ = fmt.Sprintf("%x", &i) // %x still takes a pointer
	_ = fmt.Sprintf("%o", &i) // so does %o
	_ = fmt.Sprintf("%5d", &i)
	_ = fmt.Sprintf("%[2]d %[1]d", &i, i)
	_ = fmt.Sprintf("%d", unsafe.Pointer(&i))
}

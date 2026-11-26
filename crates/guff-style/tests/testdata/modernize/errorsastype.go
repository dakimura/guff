//go:build go1.26

package errorsastype

import (
	"errors"
	"os"
)

var packagePathErr *os.PathError

func _(err error) {
	if errors.As(err, &packagePathErr) { // package-level: skip
		print(packagePathErr)
	}

	{
		var patherr *os.PathError
		if errors.As(err, &patherr) { // want
			print(patherr)
		}
	}
	{
		var patherr *os.PathError
		print("not a use of patherr")
		if errors.As(err, &patherr) { // want
			print(patherr)
		}
		print("also not a use of patherr")
	}
	{
		var patherr *os.PathError
		if errors.As(err, &patherr) { // want (unused in body → _)
			print("not a use of patherr")
		}
	}
	{
		var patherr *os.PathError
		print(patherr)
		if errors.As(err, &patherr) { // used before if: skip
			print(patherr)
		}
	}
	{
		var patherr *os.PathError
		if errors.As(err, &patherr) { // used after if: skip
			print(patherr)
		}
		print(patherr)
	}

	const ok = 1
	{
		var patherr *os.PathError
		if errors.As(err, &patherr) { // want (shadow ok)
			print(patherr)
		}
	}
	{
		var patherr *os.PathError
		if errors.As(err, &patherr) { // want (fresh ok name)
			print(patherr, ok)
		}
	}
	{
		var patherr *os.PathError
		if !errors.As(err, &patherr) { // want negated
			print(patherr)
		}
	}
	{
		var patherr *os.PathError
		var linkerr *os.LinkError
		if errors.As(err, &patherr) { // want
			print(patherr)
		} else if !errors.As(err, &linkerr) { // want
			print("not a use of linkerr")
		}
	}
	{
		var patherr *os.PathError
		if !errors.As(err, &patherr) { // want
			print("not a use of patherr")
		} else {
			print(patherr)
		}
	}
	{
		var patherr *os.PathError = &os.PathError{}
		if !errors.As(err, &patherr) { // initialized: skip
			print(patherr)
		}
	}
	{
		type Foo interface {
			Bar() string
		}
		var target Foo
		if errors.As(err, &target) { // does not implement error: skip
			print(target)
		}
	}
	{
		type FooError interface {
			Bar() string
			error
		}
		var target FooError
		if errors.As(err, &target) { // want
			print(target)
		}
	}
}

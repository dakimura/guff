package main

import "fmt"

type strStringer string

func (s strStringer) String() string { return "x" }

type bytesStringer []byte

func (s bytesStringer) String() string { return "x" }

type structStringer struct{ n int }

func (s *structStringer) String() string { return "x" }

type plainStr string

type plainBytes []byte

// The Stringer branch comes first, before either string branch and before the
// byte-slice one. Each of these four used to fall through it, because the test
// was "is this type *named* fmt.Stringer" rather than "does it implement it".

// A named string type with a String method: the Stringer branch, not the
// underlying-string one. boundary's credential.Password is this shape.
func stringWithStringMethod(v strStringer) string { return fmt.Sprintf("%s", v) }

// A named byte slice with a String method: nothing else reported this at all.
func byteSliceWithStringMethod(v bytesStringer) string { return fmt.Sprintf("%s", v) }

// A struct pointer with a String method: no other branch could apply.
func structPointerWithStringMethod(v *structStringer) string { return fmt.Sprintf("%s", v) }

// The interface itself — the one shape the old name test did catch.
func theInterfaceItself(v fmt.Stringer) string { return fmt.Sprintf("%s", v) }

// Below the Stringer branch, in upstream's order.

func alreadyAString(v string) string { return fmt.Sprintf("%s", v) }

func underlyingIsAString(v plainStr) string { return fmt.Sprintf("%s", v) }

func namedByteSlice(v plainBytes) string { return fmt.Sprintf("%s", v) }

func plainByteSlice(v []byte) string { return fmt.Sprintf("%s", v) }

type ptrFormatter struct{}

func (p *ptrFormatter) Format(f fmt.State, verb rune) {}
func (p ptrFormatter) String() string                 { return "x" }

// Passed by value, and the value's method set has no Format — so the skip does
// not apply and the Stringer branch reports. The pointer form is in ok.go. This
// pair is what pins the skip to the argument's own method set rather than to
// the addressable one.
func formatOnPointerPassedByValue(v ptrFormatter) string { return fmt.Sprintf("%s", v) }

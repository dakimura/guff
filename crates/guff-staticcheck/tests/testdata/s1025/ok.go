package main

import "fmt"

type formatter struct{}

func (formatter) Format(f fmt.State, verb rune) {}
func (formatter) String() string                { return "x" }

type ptrFormatter struct{}

func (p *ptrFormatter) Format(f fmt.State, verb rune) {}
func (p ptrFormatter) String() string                 { return "x" }

// A type that handles %s itself is skipped before either branch below it. This
// one is the reason the Stringer fix cannot land alone: it was silent only
// because the Stringer branch never fired.
func hasFormat(v formatter) string { return fmt.Sprintf("%s", v) }

// The pointer's method set does have Format, so this is skipped. Its value
// counterpart is in bad.go: the skip reads the method set of the argument's own
// type, exactly as `msCache.MethodSet(T)` does, so a pointer-receiver Format
// does not cover a value.
func hasFormatOnThePointer(v *ptrFormatter) string { return fmt.Sprintf("%s", v) }

// No branch applies: an error has Error(), not String(), and is neither a
// string nor a byte slice. Type-correct on purpose — an int with %s would be a
// printf error, and this fixture is about S1025.
func anError(v error) string { return fmt.Sprintf("%s", v) }

// A different verb.
func otherVerb(v string) string { return fmt.Sprintf("%q", v) }

// More than one argument.
func twoArgs(a, b string) string { return fmt.Sprintf("%s%s", a, b) }

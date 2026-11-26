package main

import (
	"fmt"
	"log"
	"os"
	"testing"
)

func f() { fmt.Print(fmt.Sprintf("hi")) }

// Upstream's three arms, measured against staticcheck 2.12.2 on 2026-09-21.
// The one line above is `fmt.Print` and nothing else: every shape below was
// silent, or spoke with the wrong words, while the golden stayed green.

func plain(err error, format string) {
	fmt.Print(fmt.Sprintf("a %v", err))
	fmt.Println(fmt.Sprintf("a %v", err))
	_ = fmt.Sprint(fmt.Sprintf("a %v", err))
	_ = fmt.Sprintln(fmt.Sprintf("a %v", err))
	// Silent: a non-literal format, so the caller cannot promise a newline.
	fmt.Println(fmt.Sprintf(format, err))
	// Silent: `[x]` is an exact arity.
	fmt.Print(fmt.Sprintf("a %v", err), "b")
}

// `fmt.Fprint`/`fmt.Fprintln` are the `[_ x]` arm: the writer is the `_`.
func writers(err error) {
	fmt.Fprint(os.Stderr, fmt.Sprintf("a %v", err))
	fmt.Fprintln(os.Stderr, fmt.Sprintf("a %v", err))
	// Silent: the Sprintf has to be the second argument, not any argument.
	fmt.Fprintln(os.Stderr, "x", fmt.Sprintf("a %v", err))
}

func pkgLog(err error) {
	log.Print(fmt.Sprintf("a %v", err))
	log.Println(fmt.Sprintf("a %v", err))
	log.Fatal(fmt.Sprintf("a %v", err))
	log.Fatalln(fmt.Sprintf("a %v", err))
	log.Panic(fmt.Sprintf("a %v", err))
	log.Panicln(fmt.Sprintf("a %v", err))
}

// The *methods* of the same package's Logger. Their `FuncName` is
// `(*log.Logger).Print`, not `log.Print`, and the message names the receiver
// as it was written.
func loggerMethods(l *log.Logger, err error) {
	l.Print(fmt.Sprintf("a %v", err))
	l.Println(fmt.Sprintf("a %v", err))
	l.Fatal(fmt.Sprintf("a %v", err))
	l.Fatalln(fmt.Sprintf("a %v", err))
	l.Panic(fmt.Sprintf("a %v", err))
	l.Panicln(fmt.Sprintf("a %v", err))
}

func testingMethods(t *testing.T, tb testing.TB, err error) {
	t.Error(fmt.Sprintf("a %v", err))
	t.Fatal(fmt.Sprintf("a %v", err))
	t.Log(fmt.Sprintf("a %v", err))
	t.Skip(fmt.Sprintf("a %v", err))
	tb.Error(fmt.Sprintf("a %v", err))
	// Silent: `Errorf` is not a key of the mapping.
	t.Errorf(fmt.Sprintf("a %v", err))
}

type svc struct{ logger *log.Logger }

type loggers struct{ list []*log.Logger }

// The receiver is rendered as written, whatever shape it has.
func nontrivialReceivers(s svc, ls loggers, err error) {
	s.logger.Print(fmt.Sprintf("a %v", err))
	ls.list[0].Print(fmt.Sprintf("a %v", err))
}

type ownError struct{}

func (ownError) Error(args ...any) {}

// Silent: not `testing.TB`, so its `FuncName` is not in the mapping.
func userType(o ownError, err error) {
	o.Error(fmt.Sprintf("a %v", err))
}

type shadowed struct{ *testing.T }

func (shadowed) Errorf(format string, args ...any) {}

// Silent: "ensure that Errorf/Fatalf refer to the right method" — here
// `Errorf` is the wrapper's own, not `(*testing.common).Errorf`.
func shadowedErrorf(w shadowed, err error) {
	w.Error(fmt.Sprintf("a %v", err))
}

type plainWrap struct{ *testing.T }

// Reported: the same embedding without the redefinition.
func notShadowed(w plainWrap, err error) {
	w.Error(fmt.Sprintf("a %v", err))
}

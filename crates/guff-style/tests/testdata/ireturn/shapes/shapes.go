package shapes

import (
	"io"

	"ireturnshapes/sub"
)

// Through a selector, the interface's own name decides: not in upstream's
// stdlib table, so reported.
func viaSelector() sub.Iface { return nil }

// An alias is a named interface of *this* package, whatever it aliases —
// upstream classifies by `TypeOf(ident).String()`, which is the alias's name.
type local = sub.Iface

func viaAlias() local { return nil }

type errAlias = error

func viaErrorAlias() errAlias { return nil }

type readerAlias = io.Reader

func viaStdAlias() readerAlias { return nil }

// Allowed by default: stdlib through a selector, `error`, `any`.
func stdSelector() io.Reader { return nil }

func plainError() error { return nil }

func plainAny() any { return nil }

// Not results to ireturn at all: only an identifier, a selector or an
// interface literal is looked at, so a pointer or an instantiation is not.
func pointerToIface() *sub.Iface { return nil }

func instantiated() sub.Gen[int] { return nil }

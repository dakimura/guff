package stringer

import (
	"fmt"
	"log/slog"
)

// loggercheck v0.12: a pointer value whose element type implements
// fmt.Stringer may panic in String() when nil.

type ValStringer struct{ n int }

func (v ValStringer) String() string { return fmt.Sprint(v.n) }

type PtrStringer struct{ n int }

func (p *PtrStringer) String() string { return fmt.Sprint(p.n) }

type NotStringer struct{ n int }

type WrongStringer struct{}

func (WrongStringer) String() int { return 0 }

type Alias = ValStringer

func values(v *ValStringer, p *PtrStringer, n *NotStringer, s fmt.Stringer, w *WrongStringer, a *Alias, vv ValStringer) {
	slog.Info("msg", "v", v)   // reported: element implements Stringer
	slog.Info("msg", "p", p)   // not: only the pointer implements it
	slog.Info("msg", "n", n)   // not: no String at all
	slog.Info("msg", "s", &s)  // not: pointer to an interface has no methods
	slog.Info("msg", "w", w)   // not: String() int is not fmt.Stringer
	slog.Info("msg", "a", a)   // reported: through the alias
	slog.Info("msg", "vv", vv) // not: not a pointer
	slog.Info("msg", v, "k")   // the key position is not a value
}

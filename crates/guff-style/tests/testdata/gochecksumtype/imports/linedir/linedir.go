// Package linedir puts a sum type and a switch behind a //line directive: the
// "(from ...)" location is fset.Position — adjusted, whatever the file — while
// golangci reverts an issue's own position when it lands outside a .go file.
package linedir

//sumtype:decl
type Plain interface{ plain() }

type P1 struct{}

func (P1) plain() {}

type P2 struct{}

func (P2) plain() {}

//line gen.tmpl:100
//sumtype:decl
type Tmpl interface{ tmpl() }

type T1 struct{}

func (T1) tmpl() {}

type T2 struct{}

func (T2) tmpl() {}

func tmplSwitch(t Tmpl, p Plain) {
	switch t.(type) {
	case T1:
	}
	switch p.(type) {
	case P1:
	}
}

// A `//line` naming a .go file after the package clause is left out: golangci
// keeps such an issue on the adjusted name (`filename_unadjuster` maps only
// files whose package clause is adjusted), guff's generic
// `unadjusted_position` reverts it — a deviation of every linter, not of this
// one (see crates/guff-lint/src/exclude.rs).

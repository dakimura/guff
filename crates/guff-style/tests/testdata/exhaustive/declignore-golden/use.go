package use

// The same declarations seen from another package: in-run (facts) and from a
// module the run never analyses (guff reads that module's source for the
// directives, as golangci-lint runs the analyzer over dependencies).

import (
	dep "example.com/enumdep/declignore"
	depenum "example.com/enumdep/enum"
	local "example.com/exhaustiveignore/decl"
	localenum "example.com/exhaustiveignore/enum"
)

func _(
	k dep.Kind, b dep.Base, i dep.Ignored, bt dep.BadType, bs dep.BadSpec,
	gs dep.GoodSpecIgnored, c dep.Conflict, ss dep.SkippedSpec, e dep.Enforced,
) {
	switch k {
	}
	switch b {
	}
	switch i {
	}
	switch bt {
	}
	switch bs {
	}
	switch gs {
	}
	switch c {
	}
	switch ss {
	}
	switch e {
	}
}

func _(
	k local.Kind, b local.Base, i local.Ignored, bt local.BadType, bs local.BadSpec,
	gs local.GoodSpecIgnored, c local.Conflict, ss local.SkippedSpec, e local.Enforced,
) {
	switch k {
	}
	switch b {
	}
	switch i {
	}
	switch bt {
	}
	switch bs {
	}
	switch gs {
	}
	switch c {
	}
	switch ss {
	}
	switch e {
	}
}

func _(
	a depenum.DeclGroupIgnoredEnum,
	b depenum.DeclIgnoredEnum,
	c depenum.DeclTypeIgnoredEnum,
	d depenum.DeclTypeInnerIgnore,
	e depenum.DeclTypeInnerNotIgnore,
	f depenum.DeclTypeIgnoredValue,
	g depenum.DeclTypePartialIgnore,
) {
	switch a {
	}
	switch b {
	}
	switch c {
	}
	switch d {
	}
	switch e {
	}
	switch f {
	}
	switch g {
	}
}

func _(
	a localenum.DeclGroupIgnoredEnum,
	b localenum.DeclIgnoredEnum,
	c localenum.DeclTypeIgnoredEnum,
	d localenum.DeclTypeInnerIgnore,
	e localenum.DeclTypeInnerNotIgnore,
	f localenum.DeclTypeIgnoredValue,
	g localenum.DeclTypePartialIgnore,
) {
	switch a {
	}
	switch b {
	}
	switch c {
	}
	switch d {
	}
	switch e {
	}
	switch f {
	}
	switch g {
	}
}

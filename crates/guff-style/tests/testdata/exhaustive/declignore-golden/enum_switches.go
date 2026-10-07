package enum

// Added next to exhaustive v0.13.0's own testdata/src/enum files: upstream
// checks their members through `// want T:"..."` fact assertions, which the
// golden tier cannot read, so an empty switch over each type spells the
// member set out in its message instead.

func _(
	a DeclGroupIgnoredEnum,
	b DeclIgnoredEnum,
	c DeclTypeIgnoredEnum,
	d DeclTypeInnerIgnore,
	e DeclTypeInnerNotIgnore,
	f DeclTypeIgnoredValue,
	g DeclTypePartialIgnore,
	h IotaEnum,
	i PkgRequireSameLevel,
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
	switch h {
	}
	switch i {
	}
}

// Function-local enums from scope.go's shapes: upstream walks every
// declaration, not just the file's top level.
func _() {
	type T uint
	const (
		C T = iota
		D
	)
	var t T
	switch t {
	}

	for {
		type Inner uint
		const (
			_  Inner = 100
			IX Inner = 200
		)
		var in Inner
		switch in {
		}
		break
	}
}

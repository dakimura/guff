// Package directives exercises revive's own `//revive:disable` comments.
//
// A directive turns a rule off from its line to the end of the file, or for a
// single line with `-line` / `-next-line`; an `enable` closes the interval a
// `disable` opened. Naming no rule applies it to every enabled rule.
package directives

// DirectivesKept stutters and nothing exempts it.
type DirectivesKept struct{ ID int64 }

// DirectivesLine is exempted by a trailing line directive.
type DirectivesLine struct{ ID int64 } //revive:disable-line:exported

//revive:disable-next-line:exported
// DirectivesNextLine is exempted by the directive above it.
type DirectivesNextLine struct{ ID int64 }

//revive:disable:exported

// DirectivesBlock is inside a disabled block.
type DirectivesBlock struct{ ID int64 }

//revive:enable:exported

// DirectivesAfterEnable stutters again once the block closes.
type DirectivesAfterEnable struct{ ID int64 }

// DirectivesOtherRule keeps its finding: the directive names a different rule.
type DirectivesOtherRule struct{ ID int64 } //revive:disable-line:var-naming

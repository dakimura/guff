// Package exflags has one undocumented exported declaration of every kind the
// `exported` rule can be told to skip.
//
// Upstream's `Configure` reads seven flags:
//
//	r.disabledChecks = disabledChecks{PrivateReceivers: true, PublicInterfaces: true}
//	r.isRepetitiveMsg = "stutters"
//	…
//	case isRuleOption(flag, "checkPrivateReceivers"):        PrivateReceivers = false
//	case isRuleOption(flag, "disableStutteringCheck"):       RepetitiveNames = true
//	case isRuleOption(flag, "sayRepetitiveInsteadOfStutters"): isRepetitiveMsg = "is repetitive"
//	case isRuleOption(flag, "checkPublicInterface"):         PublicInterfaces = false
//	case isRuleOption(flag, "disableChecksOnConstants"):     Const = true
//	case isRuleOption(flag, "disableChecksOnFunctions"):     Function = true
//	case isRuleOption(flag, "disableChecksOnMethods"):       Method = true
//	case isRuleOption(flag, "disableChecksOnTypes"):         Type = true
//	case isRuleOption(flag, "disableChecksOnVariables"):     Var = true
//
// guff read two of them. telegraf writes four, and `disable-checks-on-types`
// alone was 312 findings golangci-lint does not make.
package exflags

type Undocumented struct{ N int }

const UndocumentedConst = 1

var UndocumentedVar = 2

func UndocumentedFunc() {}

func (u Undocumented) UndocumentedMethod() {}

type unexportedRecv struct{ N int }

// An exported method on an unexported receiver: skipped unless
// `checkPrivateReceivers`.
func (u unexportedRecv) ExportedOnPrivate() {}

// ExflagsThing starts with the package name, so it is the repetitive-name
// finding — and the one whose wording `sayRepetitiveInsteadOfStutters` changes.
// `checkRepetitiveNames` is not behind `isDisabled("type")`, so it survives
// `disableChecksOnTypes`.
type ExflagsThing struct{ N int }

// PublicIface is documented; its method is not, which only
// `checkPublicInterface` asks about.
type PublicIface interface {
	Undoc() int
}

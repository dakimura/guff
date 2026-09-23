// Package unexportedreturnalias covers unexported-return's cross-package gate.
//
// revive type-checks the files itself with `importer.Default()`, which
// resolves no import in a module build, so a return type that can only be
// spelled through an import comes back nil and `exportedType(nil)` answers
// "exported". An alias to another package's type is exactly that shape:
// datadog-agent's dogstatsd reader declares `type resource = metrics.Resource`
// and returns `[]*resource`.
//
// The line is the alias *target*, not "anything that touches an import":
// `localStruct` below holds an imported field, still type-checks locally, and
// is still reported.
//
// Measured against golangci-lint 2.12.2 (revive v1.15.0).
package unexportedreturnalias

import (
	"bytes"

	"example.com/revive/vardeclother"
)

type crossAlias = vardeclother.Case // alias to another package's type
type stdAlias = bytes.Buffer        // alias to a stdlib type
type aliasChain = crossAlias        // an alias of such an alias
type localAlias = Exported          // alias to a local exported type
type hidden struct{ B int }         // a plain unexported type

type localStruct struct{ inner vardeclother.Case } // unexported, resolvable here

// Exported is the receiver: unexported-return only looks at exported methods
// of exported types.
type Exported struct{ C int }

// CrossSlice is silent: the alias target lives in another package.
func (e *Exported) CrossSlice() []*crossAlias { return nil }

// CrossPointer is silent for the same reason.
func (e *Exported) CrossPointer() *crossAlias { return nil }

// CrossMap is silent: the gate follows the map's element.
func (e *Exported) CrossMap() map[string]*crossAlias { return nil }

// StdAlias is silent: the standard library is an import like any other.
func (e *Exported) StdAlias() *stdAlias { return nil }

// AliasChain is silent: the chain ends in another package.
func (e *Exported) AliasChain() *aliasChain { return nil }

// LocalAlias is reported: the alias resolves without an import.
func (e *Exported) LocalAlias() *localAlias { return nil }

// Hidden is reported: a plain unexported type.
func (e *Exported) Hidden() *hidden { return nil }

// LocalStruct is reported: holding an imported field is not the same as being
// spelled through one.
func (e *Exported) LocalStruct() *localStruct { return nil }

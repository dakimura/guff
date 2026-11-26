// Package oldmethod holds deprecated *methods* reached through embedding —
// goldmark's shape: `ast.CodeSpan` embeds `BaseInline`, which embeds
// `BaseNode`, and `BaseNode.Text` is the deprecated one. The importer's source
// scan keys a method by its declaring type (`Base.Text`); a lookup keyed by the
// selection's receiver asks for `Inline.Text` or `Deep.Text`, which nothing
// writes.
package oldmethod

import "example.com/oldinner"

type Base struct{}

// Text returns text.
//
// Deprecated: do not use.
func (b *Base) Text() []byte { return nil }

// Val is deprecated on a value receiver.
//
// Deprecated: do not use.
func (b Base) Val() int { return 0 }

// Inline promotes Base's methods one level out, Deep two.
type Inline struct{ Base }
type Deep struct{ Inline }

// PtrEmb embeds *Base: same promotion, one pointer hop.
type PtrEmb struct{ *Base }

// Own shadows the deprecated Text with a live one.
type Own struct{ Base }

// Text is not deprecated.
func (o *Own) Text() []byte { return nil }

// Node is an interface whose method is deprecated.
type Node interface {
	// Deprecated: interface method.
	Text() []byte
}

// Wrap promotes a method declared in a package the use site does not import.
type Wrap struct{ oldinner.Core }

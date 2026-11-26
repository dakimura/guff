//go:build go1.24

// Package omitzeroplain is the same shape without the marker, so the two
// nested-struct tags are findings.
package omitzeroplain

type Holder struct {
	Name  string       `json:"name"`
	Value NestedValue  `json:"value,omitempty"`
	Ref   NestedSecret `json:"ref,omitempty"`
}

type NestedValue struct{ Raw string }

type NestedSecret struct{ Name string }

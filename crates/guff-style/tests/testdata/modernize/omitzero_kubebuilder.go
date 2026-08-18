//go:build go1.24

// Package omitzerokubebuilder carries a kubebuilder marker, which turns
// `omitzero` off for the whole package: kubebuilder has its own interpretation
// of the tag (go.dev/issue/76649), so upstream reports nothing here — not even
// the fields whose tags have nothing to do with a marker. dapr's `pkg/apis/**`
// are CRD types of exactly this shape, which was 24 findings.
package omitzerokubebuilder

// NameValue is a property.
//
//+kubebuilder:object:generate=true
type NameValue struct {
	Name string `json:"name"`

	//+optional
	Value DynamicValue `json:"value,omitempty"`

	//+optional
	Ref SecretRef `json:"ref,omitempty"`
}

// DynamicValue is a nested struct.
type DynamicValue struct{ Raw string }

// SecretRef is another one.
type SecretRef struct{ Name string }

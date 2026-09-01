// Package g117 is gosec's G117: an exported struct field that looks like a
// secret, reaching a JSON / YAML / XML / TOML serializer.
//
// The rule fires on the *marshal call*, so every function here is one call and
// is marked `// fires` or `// silent`. The silent ones carry the rule: four
// separate gates decide that the field will not actually be serialized, and a
// port that implements only the field-name match reports all of them.
package g117

import (
	"bytes"
	"encoding/json"
	"encoding/xml"

	"github.com/BurntSushi/toml"
	yaml "gopkg.in/yaml.v3"
)

type creds struct {
	Password string `json:"password"`
	Name     string `json:"name"`
}

// fires — the field name matches the default pattern.
func JSONMarshal() { _, _ = json.Marshal(creds{}) }

// fires — `MarshalIndent` is the second function sink of the JSON format.
func JSONMarshalIndent() { _, _ = json.MarshalIndent(creds{}, "", " ") }

// fires — the method sink: `Encode` on a `*json.Encoder`.
func JSONEncode(w *bytes.Buffer) { _ = json.NewEncoder(w).Encode(creds{}) }

type renamed struct {
	Secret string `json:"harmless"`
	Other  string `json:"password"`
}

// fires — and it names `Secret`, not `Other`: the field name is matched as well
// as the serialized key, and the first matching field in declaration order
// wins. The reported key is the one the tag gives, `harmless`.
func TagRenamedToSecret() { _, _ = json.Marshal(renamed{}) }

type omitted struct {
	Password string `json:"-"`
	Name     string `json:"name"`
}

// silent — `json:"-"` keeps the field out of the output.
func TagOmitted() { _, _ = json.Marshal(omitted{}) }

type unexported struct {
	password string
	Name     string
}

// silent — an unexported field is not serialized.
func Unexported() { _, _ = json.Marshal(unexported{}) }

var _ = unexported{password: ""}

type nonString struct {
	Password int
	Name     string
}

// silent — an `int` cannot hold a secret as far as the rule is concerned.
func NonString() { _, _ = json.Marshal(nonString{}) }

type byteSecret struct {
	Token []byte `json:"token"`
}

// silent, and not because of the `[]byte`: a bare `Token` does not match the
// default pattern, whose token alternatives all want a prefix (`api_token`,
// `access-token`, …). `[]byte` *is* a secret candidate type; this is the shape
// that proves which half is doing the work.
func ByteSliceNamedToken() { _, _ = json.Marshal(byteSecret{}) }

type nested struct {
	Inner creds
}

// silent — only the type's own fields are examined, and a struct field is not
// a secret candidate type. The walk does not descend into `creds`.
func Nested() { _, _ = json.Marshal(nested{}) }

type embedsSecret struct {
	creds
	Name string
}

// silent for the same reason: a promoted field belongs to the embedded struct.
func EmbeddedSecretField() { _, _ = json.Marshal(embedsSecret{}) }

// fires — a pointer to the struct is unwrapped.
func Pointer() { _, _ = json.Marshal(&creds{}) }

// fires — so is a slice of it.
func SliceOf() { _, _ = json.Marshal([]creds{}) }

// fires — and a map's *value* type.
func MapOf() { _, _ = json.Marshal(map[string]creds{}) }

type marshalerType struct {
	Password string
}

func (m marshalerType) MarshalJSON() ([]byte, error) { return nil, nil }

// silent — the type serializes itself, so its fields are never reached.
func CustomMarshaler() { _, _ = json.Marshal(marshalerType{}) }

type ptrMarshaler struct {
	Password string
}

func (m *ptrMarshaler) MarshalJSON() ([]byte, error) { return nil, nil }

// silent — upstream asks the *pointer* method set, so a pointer-receiver
// marshaler counts even when a value is marshalled.
func PtrMarshalerByValue() { _, _ = json.Marshal(ptrMarshaler{}) }

type marshalerBase struct{}

func (marshalerBase) MarshalJSON() ([]byte, error) { return nil, nil }

type embedsMarshaler struct {
	marshalerBase
	Password string
}

// silent — the method set includes methods promoted from embedded fields.
func EmbeddedMarshaler() { _, _ = json.Marshal(embedsMarshaler{}) }

// silent — the literal passes a call result for the field, which reads as
// masking it before serialization.
func Transformed(mask func(string) string) {
	_, _ = json.Marshal(creds{Password: mask("x")})
}

type wrapper struct {
	Password string
}

// silent — the call is inside a custom marshaler, where the author is
// controlling serialization by hand.
func (w wrapper) MarshalJSON() ([]byte, error) {
	type alias struct {
		Password string
	}

	return json.Marshal(alias{})
}

// fires — XML is its own format with its own tag key, and with no `xml` tag the
// key reported is the field name.
func XMLMarshal() { _, _ = xml.Marshal(creds{}) }

// fires — YAML, through an aliased import.
func YAMLMarshal() { _, _ = yaml.Marshal(creds{}) }

// fires — TOML has no marshaler interface and reaches the rule only through
// `Encoder.Encode`.
func TOMLEncode(w *bytes.Buffer) { _ = toml.NewEncoder(w).Encode(creds{}) }

type twoSecrets struct {
	Name     string
	Password string
	Secret   string
}

// fires once, naming the first match in declaration order.
func TwoSecrets() { _, _ = json.Marshal(twoSecrets{}) }

type taggedComma struct {
	Password string `json:"pass,omitempty"`
}

// fires — the key is the part of the tag before the comma.
func TaggedComma() { _, _ = json.Marshal(taggedComma{}) }

type emptyTagName struct {
	Password string `json:",omitempty"`
}

// fires — a tag with no name falls back to the field name.
func EmptyTagName() { _, _ = json.Marshal(emptyTagName{}) }

// fires — the argument does not have to be a literal.
func FromVariable(c creds) { _, _ = json.Marshal(c) }

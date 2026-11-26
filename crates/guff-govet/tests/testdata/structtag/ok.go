package ok

type Meta struct{}

type T struct {
	X int `json:"name"`
	// Embedded fields with encoding tags are skipped (go vet structtag).
	Meta `json:",inline"`
	// Escaped quotes inside a tag value are valid (reflect.StructTag / go vet).
	EnableRestore bool `option:"enable-restore" help:"requires \"s3-restore\" feature flag"`
	// XML may contain a single space in the name (DAV: …); JSON may too.
	Quota int `xml:"DAV: response>propstat>prop>quota-available-bytes"`
	Info  string `json:"File INFO"`
}


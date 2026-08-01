package ok

type Meta struct{}

type T struct {
	X int `json:"name"`
	// Embedded fields with encoding tags are skipped (go vet structtag).
	Meta `json:",inline"`
}


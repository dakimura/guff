package tagalign

// tagalign v1.4.4: a tagged inline struct field after a gap starts the next
// group (v1.4.3 skipped it, so the field below it formed a group alone).
type GapStruct struct {
	A string `json:"a" yaml:"a"`

	B struct {
		C string `json:"c"`
	} `json:"b" yaml:"b"`
	DLonger string `yaml:"d" json:"d"`
}

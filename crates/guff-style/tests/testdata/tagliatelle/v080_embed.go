package tagliatelle

// tagliatelle v0.8.0: a json/v2 `embed` field has no name of its own to lint;
// the same tag without `embed` is still checked.
type Embedded struct {
	Inner    struct{ A int } `json:"Inner_Value,embed"`
	NotEmbed struct{ A int } `json:"Inner_Value"`
}

package embeddedstructfieldcheck

type EmbedMe struct {
	N int
}

type NoSpaceStruct struct {
	EmbedMe
	version int
}

type NotSortedStruct struct {
	version int

	EmbedMe
}

type MixedEmbeddedAndNotEmbedded struct {
	EmbedMe

	name string

	EmbedMe2

	age int
}

type EmbedMe2 struct {
	X int
}

type EmbeddedWithPointers struct {
	*EmbedMe
	version int
}

type ValidStructWithTags struct {
	EmbeddedWithPointers `json:"foo"`
	NoSpaceStruct        `json:"bar"`

	D string
}

type StructWithTagsNoSpace struct {
	EmbeddedWithPointers `json:"foo"`
	NoSpaceStruct        `json:"bar"`
	D                    string
}

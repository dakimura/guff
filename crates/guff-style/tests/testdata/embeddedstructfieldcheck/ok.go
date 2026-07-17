package ok

type EmbedMe struct {
	N int
}

type ValidStruct struct {
	EmbedMe

	version int
}

type OnlyEmbedded struct {
	EmbedMe
}

type OnlyRegular struct {
	version int
	name    string
}

type ValidStructWithTags struct {
	EmbedMe `json:"foo"`

	D string
}

type PointerEmbed struct {
	*EmbedMe

	version int
}

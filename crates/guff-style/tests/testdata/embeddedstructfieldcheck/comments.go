package comments

type EmbedMe struct {
	N int
}

// Blank line + doc before regular field must pass (upstream comments-empty-line).
type ValidStructWithSingleLineComments struct {
	// EmbedMe Single line comment
	EmbedMe

	// version Single line comment
	version int
}

// Doc immediately after embedded field (no blank) must fail.
type StructWithSingleLineComments struct {
	// EmbedMe Single line comment
	EmbedMe
	// version Single line comment
	version int
}

type StructWithMultiLineComments struct {
	// EmbedMe Single line comment
	EmbedMe
	// version Single line comment
	// very long comment
	version int
}

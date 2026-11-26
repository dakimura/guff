package store

// S is what vault.CredentialStore embeds. boundary's real one is a protobuf
// message; all that matters here is that the promoted method exists.
type S struct {
	PublicId string
}

func (s *S) GetPublicId() string { return s.PublicId }

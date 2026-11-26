package tool

// T is embedded in a test-only type that session exports, so a session built
// without tool in scope loses the promoted method rather than the import — the
// same way boundary's vault.CredentialStore loses GetPublicId when its own
// store dependency is stranded.
type T struct{}

func (T) Use() string { return "tool" }

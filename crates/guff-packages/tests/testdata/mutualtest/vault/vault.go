package vault

import "example.com/mutual/store"

// CredentialStore promotes GetPublicId from the embedded *store.S. If the seed
// builds this package without store in scope, the embedded field is invalid and
// every consumer loses the promoted method rather than the import.
type CredentialStore struct {
	*store.S
}

func New(id string) *CredentialStore {
	return &CredentialStore{S: &store.S{PublicId: id}}
}

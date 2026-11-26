package gosec_scores

import "crypto/md5"

// G101 is High severity but only Low confidence, so `confidence: medium`
// drops it while keeping G401 (Medium/High) from the md5 call below.
const fixtureStorePasswordSecretName = "fixture-zq7kv2mxw9pt4rj8nb3ch-db-passwd"

// Matches the `(?i)example` pattern override but not the default name list.
const exampleValue = "fixture-zq7kv2mxw9pt4rj8nb3ch-db-passwd"

func weakHash() {
	_ = md5.New()
}

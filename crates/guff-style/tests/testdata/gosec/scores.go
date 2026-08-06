package gosec_scores

import "crypto/md5"

// G101 is High severity but only Low confidence, so `confidence: medium`
// drops it while keeping G401 (Medium/High) from the md5 call below.
const dataplatformPasswordSecretName = "merpay-dataplatform-jp-alloydb-password"

// Matches the `(?i)example` pattern override but not the default name list.
const exampleValue = "merpay-dataplatform-jp-alloydb-password"

func weakHash() {
	_ = md5.New()
}

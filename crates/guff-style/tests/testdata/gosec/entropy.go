// Package entropy exercises G101's zxcvbn entropy gate from both sides.
//
// The name pattern matches every constant here; what decides the finding is
// `isHighEntropyString`, which runs zxcvbn over the first 16 bytes and asks for
// `Entropy >= 80 || (Entropy >= 40 && Entropy/len >= 3.0)`. zxcvbn's estimate
// is dictionary-based, so a credential spelled out of English words scores low
// however long it is, and one that isn't scores high at half the length.
package entropy

// Reported: nothing in the dictionaries covers much of these.
const (
	credentialsPath = "/var/run/dapr/credentials" // 77.833
	apiTokenEnvVar  = "DAPR_API_TOKEN"            // 55.449
	appAPITokenEnv  = "APP_API_TOKEN"             // 49.567
	apiTokenHeader  = "dapr-api-token"            // 50.449
)

// Not reported: words, so the entropy stays under both thresholds.
const (
	secretStoreNameParam = "secretStoreName"          // 26.148
	mockSecretStore      = "mockSecretStore"          // 28.713
	secretKeyName        = "NAME1:SECRETKEY1"         // 34.884
	localSecretStore     = "local-secret-store"       // 36.087
	timestampWTZ         = "timestamp with time zone" // 39.777
	shortKey             = "key"                      // below minEntropyLength
)

func Use() []string {
	return []string{
		credentialsPath, apiTokenEnvVar, appAPITokenEnv, apiTokenHeader,
		secretStoreNameParam, mockSecretStore, secretKeyName, localSecretStore,
		timestampWTZ, shortKey,
	}
}

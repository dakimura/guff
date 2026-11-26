package fixtures

func foo() string {
	// allowlist ID → customId stays (not forced to customID)
	customId := "result"
	// blocklist adds VM → customVm should be customVM
	customVm := "result"
	return customId + customVm
}

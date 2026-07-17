package wsl_v5

// Used with cuddle-max-statements: 2 — two shared assigns before if are OK.
func SettingsOk() {
	a := 1
	b := 2
	if a+b > 0 {
		return
	}
}

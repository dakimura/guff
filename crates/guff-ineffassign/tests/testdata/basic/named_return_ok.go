package ok

func envCSV(name string) (ls []string) {
	if name != "" {
		ls = []string{name}
	}
	return
}

package musttag

// Custom helper that should require a `yaml` tag when configured.
func DecodeYAML(data []byte, v any) error {
	_ = data
	_ = v
	return nil
}

type Config struct {
	Host string
}

func useCustom() {
	var c Config
	_ = DecodeYAML(nil, &c)
}

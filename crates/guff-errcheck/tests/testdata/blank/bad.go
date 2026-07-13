package blank

func a() error {
	return nil
}

func b() (string, error) {
	return "", nil
}

func c() string {
	return ""
}

func main() {
	_ = a()
	a()
	b()
	c()

	{
		r, err := b()
		_ = r
		_ = err
	}

	{
		r, _ := b()
		_ = r
	}

	{
		var r, _ = b()
		_ = r
	}
}

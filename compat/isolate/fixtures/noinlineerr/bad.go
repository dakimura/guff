package p

func do() error { return nil }

func Bad() {
	if err := do(); err != nil {
		panic(err)
	}
}

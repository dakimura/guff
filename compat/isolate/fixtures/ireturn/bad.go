package p

type I interface{ M() }

func Bad() I {
	return nil
}

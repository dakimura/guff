package funlen

func Short() {
	_ = 1
	_ = 2
}

// One-liners and empty bodies must not report usize::MAX line counts.
func OneLine() string { return "ok" }

func Empty() {}


package p

func Bad() {
	_ = "this line is intentionally very long so that the lll linter reports it as exceeding the default line length limit of one hundred and twenty characters"
}

package whitespace

func multiIfBad() {
	if longConditionOne &&
		longConditionTwo {
		_ = 1
	}
}

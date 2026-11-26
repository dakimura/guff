package whitespace

func multiIfOk() {
	if longConditionOne &&
		longConditionTwo {

		_ = 1
	}
}

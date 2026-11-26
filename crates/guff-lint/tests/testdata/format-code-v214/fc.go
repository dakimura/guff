package fc

func multi0() string {
	return `line one
line two`
}

func multi1() string {
	return `line one
line two`
}

func multi2() string {
	return `line one
line two`
}

func tabs0() string {
	return "a\tb c"
}

func tabs1() string {
	return "a\tb c"
}

func tabs2() string {
	return "a\tb c"
}

func ctrl0() string {
	return "bell\a!"
}

func ctrl1() string {
	return "bell\a!"
}

func ctrl2() string {
	return "bell\a!"
}

func tick0() string {
	return "back`tick"
}

func tick1() string {
	return "back`tick"
}

func tick2() string {
	return "back`tick"
}

func plain0() string {
	return "plain value"
}

func plain1() string {
	return "plain value"
}

func plain2() string {
	return "plain value"
}

const Plain = "plain value"

func nolinted() int {
	return 1 //nolint:gocyclo // see `README`
}

func nolinted2() int {
	return 2 //nolint:gocyclo
}

// The recieve side.
func spelled() {}

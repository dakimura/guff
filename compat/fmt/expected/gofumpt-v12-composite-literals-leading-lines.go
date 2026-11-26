package p

var _ = []string{
	"foo",
}

var _ = []string{
	"foo",
}

var _ = []string{
	// joint comment
	"foo",
}

var _ = []string{
	// separate comment

	"foo",
}

var _ = map[string]string{
	"foo": "bar",
}

var _ = map[string]string{
	"foo": "bar",
}

var _ = map[string]string{
	// joint comment
	"foo": "bar",
}

var _ = map[string]string{
	// separate comment

	"foo": "bar",
}

var _ = map[string]string{
	/*
		joint comment
	*/
	"foo": "bar",
}

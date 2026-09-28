package s1005rangefunc

// A `range` over a func — an `iter.Seq` / `iter.Seq2` iterator — keeps its
// blank identifiers: upstream returns before reporting anything, because
// "iteration variables are not optional with rangefunc". Dropping the `_`
// would not compile, so there is no fix to offer. teleport ranges over
// iterators in three places and guff reported all of them.
//
// The same three shapes over a slice and a map are here too, and those *are*
// reported — a fixture with only the silent half cannot tell "the gate works"
// from "the check stopped firing".

func rangeOverSeq(seq func(yield func(int) bool)) {
	for _ = range seq {
	}
}

func rangeOverSeq2Value(seq func(yield func(int, string) bool)) {
	for k, _ := range seq {
		_ = k
	}
}

func rangeOverSeq2Both(seq func(yield func(int, string) bool)) {
	for _, _ = range seq {
	}
}

// A func value reached through a variable rather than a parameter.
func rangeOverSeqVar() {
	f := func(yield func(int) bool) {}
	for _ = range f {
	}
}

func rangeOverSlice(s []int) {
	for _ = range s {
	}
}

func rangeOverMapValue(m map[int]string) {
	for k, _ := range m {
		_ = k
	}
}

func rangeOverSliceBoth(s []int) {
	for _, _ = range s {
	}
}

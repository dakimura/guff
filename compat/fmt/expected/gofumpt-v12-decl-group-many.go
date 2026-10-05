package p

var (
	single  = "foo"
	another = "bar"
)

const (
	one   = 'q'
	two   = 'w'
	three = 'e'
	four  = 'r'
)

var not = 'a'

var v1 = 's'

//go:embed hello.txt
var v2 = 'd'

var (
	v1 = 's'
	// comment line 1
	// comment line 2
	v2 = 'd'
)

var v1 = "mixed"

const c1 = "mixed"

//go:embed hello.txt
var v1 = 's'

var (
	v2 = 'd'
	v3 = 'd'
)

// comment
var (
	v1 = 's'
	v2 = 'd'
	/* comment */
	v3 = 'd'
)

const (
	inline1 = "s1" // c1
	inline2 = "s2" // c2
	inline3 = "s3" // c3
)

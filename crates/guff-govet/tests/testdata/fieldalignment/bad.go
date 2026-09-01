// Package fieldalignment is the fixture for the analyzer of the same name —
// one of the ten `cmd/vet` leaves off, which golangci-lint runs only under
// `govet.enable-all` or an explicit `enable`.
//
// Two different diagnostics live here. One compares the struct's *size*
// against the tightest packing; the other compares its *pointer bytes* — how
// far into the object the garbage collector has to scan — which can be
// improvable even when the size is already optimal. Both wordings appear
// below, and so do the shapes that must stay silent: a struct is only
// reported when it is not already in an optimal order.
package fieldalignment

// ---- size ---------------------------------------------------------------

// bool, int64, bool: 24 bytes as written, 16 with the int64 first.
type sizeBoolInt64Bool struct {
	a bool
	b int64
	c bool
}

// The array's element alignment is what counts, not the array's own size.
type sizeWithArray struct {
	a bool
	b [3]int64
	c bool
}

// Tags do not move a field.
type sizeWithTags struct {
	A bool  `json:"a"`
	B int64 `json:"b"`
	C bool  `json:"c"`
}

type sizeWithComplex struct {
	b bool
	c complex128
	d bool
}

// An anonymous struct is a struct type of its own and is reported where it is
// written, not at the field.
type sizeAnonymous struct {
	x struct {
		a bool
		b int64
		c bool
	}
}

// ---- pointer bytes ------------------------------------------------------

// uint32 then string: the collector must scan 16 bytes, 8 if the string leads.
type ptrsUint32String struct {
	u uint32
	s string
}

// string then *uint32 scans 24; the other way round, 16.
type ptrsStringPtr struct {
	s string
	p *uint32
}

type ptrsArrayOfPtr struct {
	n bool
	p [2]*int
}

// An interface is two words and fully pointerful, so moving it first fixes
// the struct's *size* — this one reports the size wording despite living in
// the pointer section.
type ptrsInterface struct {
	b bool
	i interface{ M() }
	c bool
}

type ptrsAny struct {
	b bool
	i any
}

type ptrsSlice struct {
	b bool
	s []byte
}

type ptrsMapChanFunc struct {
	b bool
	m map[string]int
	c chan int
	f func()
}

type ptrsString struct {
	b bool
	s string
	i int64
}

// A type parameter's constraint decides its layout: `T any` is an interface,
// so it is two words wide and fully pointerful.
type ptrsGeneric[T any] struct {
	b bool
	v T
	i int64
}

// ---- already optimal, and therefore silent ------------------------------

type okOrdered struct {
	b int64
	a bool
	c bool
}

// string then uint32 already stops the scan after the string's pointer.
type okStringThenUint32 struct {
	s string
	u uint32
}

type okEmpty struct{}

type okSingleField struct {
	b bool
}

// The fields of a multi-name group are separate fields; here they already
// pack optimally, which is also the shape whose fix would have to be split
// into one name per line.
type okMultiName struct {
	a, c bool
	b    int64
}

// An embedded field is an ordinary field for layout purposes.
type mutexLike struct {
	state int32
	sema  uint32
}

type okEmbedded struct {
	mutexLike
	i int64
}

// A leading zero-sized field costs nothing.
type okLeadingZeroField struct {
	z struct{}
	i int64
	b bool
}

// A *trailing* zero-sized field is padded to one byte. The optimal order puts
// zero-sized fields first, which removes that byte — but here the padding it
// removes was already inside the struct's alignment, so both size and pointer
// bytes come out the same and nothing is reported.
type okTrailingZeroField struct {
	i int64
	b bool
	z struct{}
}

// A zero-length array of pointers has no pointer bytes at all, and this order
// is already the tightest.
type okEmptyArray struct {
	p [0]*int
	n int64
	b bool
}

// A pointer already leads, so there is nothing to gain.
type okPointerLast struct {
	p *int
	b bool
}

type okNested struct {
	b int64
	n struct {
		x int64
		y bool
	}
}

// Package ifacename is about *which* type an interface method belongs to.
//
// Upstream skips a method only when the receiver's own named type is known to
// implement an interface that requires it — `typesImplementing`, filled from
// evidence such as a `var _ I = T` assertion. guff also silenced any method
// whose name and signature matched an interface declared in the package,
// whoever implemented it, and that rule has no upstream counterpart.
//
// syncthing declares `getState() (folderState, time.Time, error)` in an
// interface that `*folder` implements by **embedding** a `stateTracker`, and it
// is `(*stateTracker).getState` that upstream reports.
package ifacename

type state int

// fires — `tracker` does not implement `folderIface` (it has no Serve), so the
// interface's declaration of `getState` says nothing about this method.
type tracker struct {
	cur     state
	changed int64
	err     error
}

func (t *tracker) getState() (current state, changed int64, err error) {
	current, changed, err = t.cur, t.changed, t.err

	return
}

type folderIface interface {
	getState() (state, int64, error)
	Serve()
}

type folder struct {
	tracker
}

func (f *folder) Serve() {}

var _ folderIface = (*folder)(nil)

func useA(t *tracker) bool {
	cur, _, _ := t.getState()

	return cur == 0
}

func useB(t *tracker) error {
	_, _, err := t.getState()

	return err
}

func useC(t *tracker) state {
	cur, _, _ := t.getState()

	return cur
}

// silent — this receiver's own type is asserted to implement the interface, so
// its signature is not the analyzer's to change.
type reader struct{ n int }

type readerIface interface {
	read() (int, int64, error)
}

func (r *reader) read() (n int, at int64, err error) {
	n, at, err = r.n, 0, nil

	return
}

var _ readerIface = (*reader)(nil)

func readA(r *reader) int {
	n, _, _ := r.read()

	return n
}

func readB(r *reader) error {
	_, _, err := r.read()

	return err
}

func readC(r *reader) int {
	n, _, _ := r.read()

	return n
}

// Upstream's httpmux testdata (x/tools v0.52.0), plus the shapes it leaves
// unpinned: constant expressions, non-constant patterns, method values and
// expressions, a ServeMux reached by value or by embedding, and patterns at
// the edges of the wildcard regexp.
package a

import "net/http"

func _() {
	http.HandleFunc("GET /x", nil)  // want "enhanced ServeMux pattern"
	http.HandleFunc("/{a}/b/", nil) // want "enhanced ServeMux pattern"
	mux := http.NewServeMux()
	mux.Handle("example.com/c/{d}", nil) // want "enhanced ServeMux pattern"
	mux.HandleFunc("/{x...}", nil)       // want "enhanced ServeMux pattern"

	// Should not match.

	// not an enhanced pattern
	http.Handle("/", nil)

	// invalid wildcard; will panic in 1.22
	http.HandleFunc("/{/a/}", nil)
	mux.Handle("/{1}", nil)
	mux.Handle("/x{a}", nil)

	// right package, wrong method
	http.ParseTime("GET /")

	// right function name, wrong package
	Handle("GET /", nil)
	HandleFunc("GET /", nil)

	// right method name, wrong type
	var sm ServeMux
	sm.Handle("example.com/c/{d}", nil)
	sm.HandleFunc("method /{x...}", nil)
}

const getRoot = "GET /"

type pattern string

const typed pattern = "/{id}"

type server struct {
	*http.ServeMux
}

func extra(dyn string, h http.Handler) {
	var mux http.ServeMux
	mux.Handle("GET /value", h)     // addressable value: same method
	(&mux).Handle("/p/{q}", h)      // explicit address
	mux.Handle(getRoot, h)          // named constant
	mux.Handle("GET "+"/concat", h) // constant expression
	mux.Handle(string(typed), h)    // conversion of a typed constant
	mux.Handle(("/paren/{p}"), h)   // parenthesised literal
	mux.Handle(dyn, h)              // not a constant
	mux.Handle(dyn+" /x", h)        // not a constant either
	mux.Handle(`/raw/{r}`, h)       // raw string
	mux.Handle("/{_}", h)           // underscore name
	mux.Handle("/{é1}", h)          // non-ASCII letter, then a digit
	mux.Handle("/{a-b}", h)         // '-' is not in the class
	mux.Handle("/{a..}", h)         // two dots is not "..."
	mux.Handle("/{a", h)            // unterminated
	mux.Handle("{a}", h)            // not after a slash
	mux.Handle("/x\tGET", h)        // tab is not a space
	mux.Handle("/a /b", h)          // any space counts

	s := server{http.NewServeMux()}
	s.Handle("GET /embedded", h)       // promoted through the embedding
	s.ServeMux.HandleFunc("/{e}", nil) // explicit field

	f := mux.Handle
	f("GET /method-value", h) // not a static call (func value)
	g := http.HandleFunc
	g("GET /func-value", nil) // nor is this

	(*http.ServeMux).Handle(&mux, "GET /expr", h) // method expression: Args[0] is the receiver
	http.Handle("POST /", h)
}

func Handle(pat string, x any)     {}
func HandleFunc(pat string, x any) {}

type ServeMux struct{}

func (*ServeMux) Handle(pat string, x any)     {}
func (*ServeMux) HandleFunc(pat string, x any) {}

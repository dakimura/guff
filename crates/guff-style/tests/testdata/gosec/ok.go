package gosec_ok

import (
	"compress/gzip"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/tls"
	"hash"
	"html/template"
	"io"
	"math/rand"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"time"
)

const safeURL = "https://example.com"

type sourceReader struct{}
func (sourceReader) Read([]byte) (int, error) { return 0, nil }

type sinkWriter struct{}
func (sinkWriter) Write([]byte) (int, error) { return 0, nil }

func ok() hash.Hash {
	return sha256.New()
}

func okListen() error {
	ln, err := net.Listen("tcp", "127.0.0.1:8080")
	if err != nil {
		return err
	}
	return ln.Close()
}

func okExec() {
	_ = exec.Command("ls", "-la")
}

func okDecompression() error {
	compressed, err := gzip.NewReader(sourceReader{})
	if err != nil {
		return err
	}
	_, err = io.CopyN(sinkWriter{}, compressed, 1<<20)
	return err
}

func okCreds() {
	password := "secret"
	_ = password
	username := "admin"
	_ = username
	txnID := "3637cfcc1eec55a50f78a7c435914583ccbc75a21dec9a0e94dfa077647146d7"
	_ = txnID
}

func okErr() error { return nil }

func okCheckedErr() error {
	if err := okErr(); err != nil {
		return err
	}
	return nil
}

func okPerms() error {
	if err := os.Mkdir("/var/lib/x", 0o750); err != nil {
		return err
	}
	if err := os.MkdirAll("/var/lib/y", 0o700); err != nil {
		return err
	}
	f, err := os.OpenFile("f", 0, 0o600)
	if err != nil {
		return err
	}
	_ = f
	if err := os.Chmod("f", 0o600); err != nil {
		return err
	}
	if err := os.WriteFile("f", nil, 0o600); err != nil {
		return err
	}
	key, err := rsa.GenerateKey(nil, 2048)
	if err != nil {
		return err
	}
	_ = key
	_ = tls.Config{InsecureSkipVerify: false}
	_ = http.Dir("/var/www")
	created, err := os.Create("/var/lib/demo")
	if err != nil {
		return err
	}
	_ = created
	resp, err := http.Get(safeURL)
	if err != nil {
		return err
	}
	_ = resp
	resp2, err := http.Get("https://example.com")
	if err != nil {
		return err
	}
	_ = resp2
	_ = &http.Server{
		Addr:              ":8080",
		ReadHeaderTimeout: 3 * time.Second,
	}
	_ = &http.Cookie{
		Name:     "session",
		Value:    "abc",
		Secure:   true,
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
	}
	bigValue, err := strconv.Atoi("30")
	if err != nil {
		return err
	}
	_ = int64(bigValue)
	_ = template.HTML("<b>ok</b>")
	return nil
}

func okTLSAssignFromField(skip bool) *tls.Config {
	cfg := new(tls.Config)
	// Upstream G402 does not match *tls.Config assignments.
	cfg.InsecureSkipVerify = skip
	return cfg
}

func okCookieLaterSecure() *http.Cookie {
	cookie := &http.Cookie{
		Name:     "session",
		Value:    "abc",
		Secure:   false,
		HttpOnly: true,
		SameSite: http.SameSiteLaxMode,
	}
	cookie.Secure = true
	return cookie
}

func okWalkNoSink() error {
	return filepath.Walk("/tmp", func(path string, info os.FileInfo, err error) error {
		_ = path
		_ = info
		return err
	})
}

func okG107NonIdentURL() {
	base := "https://example.com"
	_, _ = http.Get(base + "/ok")
	_, _ = http.Post(base+"/ok", "text/plain", nil)
}


// G404's rule list is `math/rand`'s *package-level* functions. gosec resolves
// the call by syntax (`GetCallInfo`), so a method on a `*rand.Rand` names the
// receiver's type — `*math/rand.Rand` — and matches no rule. coredns wraps
// math/rand in exactly this shape.
type safeRand struct {
	r *rand.Rand
}

func (s *safeRand) Int() int { return s.r.Int() }

func (s *safeRand) Perm(n int) []int { return s.r.Perm(n) }

// `Perm` and `Shuffle` are not on gosec's list even package-qualified.
func okPerm(n int) []int { return rand.Perm(n) }

func okShuffle(xs []int) {
	rand.Shuffle(len(xs), func(i, j int) { xs[i], xs[j] = xs[j], xs[i] })
}

// A variadic call is where upstream's taint stops: go/ssa packs the arguments
// into a slice, and `pathDependsOn` has no case for one. `filepath.Join` is
// variadic, so neither tool calls this a G122 — the tools agree, and the reason
// is an artifact of the SSA shape rather than of the path.
func g122VariadicHop() {
	_ = filepath.Walk("/tmp", func(path string, info os.FileInfo, err error) error {
		return os.Remove(filepath.Join(path, "sub"))
	})
}

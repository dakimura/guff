package gosec_bad

import (
	"compress/gzip"
	"crypto/des"
	"crypto/md5"
	"crypto/rc4"
	"crypto/rsa"
	"crypto/sha1"
	"crypto/tls"
	"database/sql"
	"html/template"
	"io"
	"math/rand"
	"net"
	"net/http"
	"net/http/cgi"
	_ "net/http/pprof"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"unsafe"

	"golang.org/x/crypto/md4"
	"golang.org/x/crypto/ripemd160"
	"golang.org/x/crypto/ssh"
)

var taintedURL = "https://example.com"

type sourceReader struct{}
func (sourceReader) Read([]byte) (int, error) { return 0, nil }

type sinkWriter struct{}
func (sinkWriter) Write([]byte) (int, error) { return 0, nil }

func bad() {
	_ = md5.New()
	_ = sha1.Sum(nil)
	_ = rand.Intn(10)
	_, _ = des.NewCipher(nil)
	_, _ = rc4.NewCipher(nil)
	_ = md4.New()
	_ = ripemd160.New()
	var x uintptr
	_ = unsafe.Pointer(x)
	_, _ = cgi.RequestFromMap(nil)

	_ = ssh.InsecureIgnoreHostKey()
	_ = http.ListenAndServe(":8080", nil)
	_, _ = net.Listen("tcp", "0.0.0.0:8080")
	_, _ = tls.Listen("tcp", ":8443", nil)
	// Resolvable through gosec's TryResolve (Obj.Decl is an AssignStmt whose
	// RHS is a literal), so upstream is silent here — kept so the golden shows
	// its absence.
	cmd := "ls"
	_ = exec.Command(cmd)
	envCmd := os.Getenv("GUFF_GOSEC_G204")
	_ = exec.Command("sh", "-c", envCmd)
	_ = exec.Command("sh", "-c", os.Getenv("GUFF_GOSEC_G204B"))

	password := "f62e5bcda4fae4f82370da0c6f20697b8f8447ef"
	_ = password
	awsKey := "AKIAI44QH8DHBEXAMPLE"
	_ = awsKey

	returnsErr()
	_ = returnsErr()

	_ = os.Mkdir("/tmp/x", 0o777)
	_ = os.MkdirAll("/tmp/y", 0o755)
	_, _ = os.OpenFile("f", 0, 0o666)
	_ = os.Chmod("f", 0o777)
	_ = os.WriteFile("f", nil, 0o644)
	_, _ = rsa.GenerateKey(nil, 1024)
	_ = tls.Config{InsecureSkipVerify: true}
	_ = http.Dir("/")
	compressed, _ := gzip.NewReader(sourceReader{})
	_, _ = io.Copy(sinkWriter{}, compressed)
	_, _ = io.CopyBuffer(sinkWriter{}, compressed, nil)

	_, _ = os.Create("/tmp/demo")
	_ = os.WriteFile("/tmp/demo2", nil, 0o644)
	_ = os.WriteFile(os.TempDir()+"/demo3", nil, 0o600)

	_, _ = http.Get(taintedURL)
	_ = (&http.Server{Addr: ":8080"}).ListenAndServe()
	_ = &http.Cookie{Name: "session", Value: "abc"}

	bigValue, _ := strconv.Atoi("2147483648")
	_ = int32(bigValue)

	a := "attacker"
	_ = template.HTML(a)

	_ = filepath.Walk("/tmp", func(path string, info os.FileInfo, err error) error {
		return os.Remove(path)
	})

	// Upstream walks back from the sink argument through calls, so a path that
	// has been through `filepath.Clean` is still the callback's path. coredns
	// `plugin/auto/walk.go` is exactly this and guff used to miss it, because
	// it matched the parameter's name and nothing derived from it.
	_ = filepath.Walk("/tmp", func(path string, info os.FileInfo, err error) error {
		cleanPath := filepath.Clean(path)
		f, err := os.Open(cleanPath)
		if err != nil {
			return err
		}
		return f.Close()
	})

	// Two non-variadic hops still count.
	_ = filepath.Walk("/tmp", func(path string, info os.FileInfo, err error) error {
		return os.Remove(filepath.ToSlash(filepath.Clean(path)))
	})

	if envPath := os.Getenv("GUFF_GOSEC_G703"); envPath != "" {
		_, _ = os.OpenFile(envPath, os.O_RDONLY, 0)
	}
}

func returnsErr() error { return nil }

// G202 — the leading literal carries a SQL keyword and an operand that cannot
// be resolved to a constant is concatenated onto it. dapr builds six of these
// out of a table name it reads at runtime.
func sqlConcatFromParam(db *sql.DB, table string) error {
	_, err := db.Exec("DELETE FROM " + table + " WHERE key = ?")
	return err
}

// The statement form, and a *sql.Tx receiver.
func sqlConcatExprStmt(tx *sql.Tx, table string) {
	tx.Exec("SELECT value FROM " + table)
}

// A *field* receiver: `getCallInfo` answers with the field's type, so the rule
// runs the same as for a local.
type sqlHolder struct{ db *sql.DB }

func sqlConcatFieldRecv(h *sqlHolder, table string) error {
	_, err := h.db.Exec("DELETE FROM " + table + " WHERE key = ?")
	return err
}

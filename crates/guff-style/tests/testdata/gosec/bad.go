package gosec_bad

import (
	"crypto/des"
	"crypto/md5"
	"crypto/rc4"
	"crypto/rsa"
	"crypto/sha1"
	"crypto/tls"
	"html/template"
	"math/rand"
	"net"
	"net/http"
	"net/http/cgi"
	_ "net/http/pprof"
	"os"
	"os/exec"
	"strconv"
	"unsafe"

	"golang.org/x/crypto/md4"
	"golang.org/x/crypto/ripemd160"
	"golang.org/x/crypto/ssh"
)

var taintedURL = "https://example.com"

func bad() {
	_ = md5.New()
	_ = sha1.Sum(nil)
	_ = rand.Intn(10)
	_ = des.NewCipher(nil)
	_ = rc4.NewCipher(nil)
	_ = md4.New()
	_ = ripemd160.New()
	var x uintptr
	_ = unsafe.Pointer(x)
	_ = cgi.RequestFromMap(nil)

	_ = ssh.InsecureIgnoreHostKey()
	_ = http.ListenAndServe(":8080", nil)
	_, _ = net.Listen("tcp", "0.0.0.0:8080")
	_, _ = tls.Listen("tcp", ":8443", nil)
	cmd := "ls"
	_ = exec.Command(cmd)

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

	_, _ = os.Create("/tmp/demo")
	_ = os.WriteFile("/tmp/demo2", nil, 0o644)
	_ = os.WriteFile(os.TempDir()+"/demo3", nil, 0o600)

	_, _ = http.Get(taintedURL)
	_ = (&http.Server{Addr: ":8080"}).ListenAndServe()

	bigValue, _ := strconv.Atoi("2147483648")
	_ = int32(bigValue)

	a := "attacker"
	_ = template.HTML(a)
}

func returnsErr() error { return nil }

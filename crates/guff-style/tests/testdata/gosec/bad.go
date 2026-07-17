package gosec_bad

import (
	"crypto/des"
	"crypto/md5"
	"crypto/rc4"
	"crypto/sha1"
	"crypto/tls"
	"math/rand"
	"net"
	"net/http"
	"net/http/cgi"
	_ "net/http/pprof"
	"os/exec"
	"unsafe"

	"golang.org/x/crypto/md4"
	"golang.org/x/crypto/ripemd160"
	"golang.org/x/crypto/ssh"
)

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
}

func returnsErr() error { return nil }

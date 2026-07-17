package gosec_ok

import (
	"crypto/rsa"
	"crypto/sha256"
	"crypto/tls"
	"hash"
	"html/template"
	"net"
	"net/http"
	"os"
	"os/exec"
	"strconv"
	"time"
)

const safeURL = "https://example.com"

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
	bigValue, err := strconv.Atoi("30")
	if err != nil {
		return err
	}
	_ = int64(bigValue)
	_ = template.HTML("<b>ok</b>")
	return nil
}

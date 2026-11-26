package pkg

import (
	"encoding/json"
	"encoding/xml"
)

type RawBytes []byte

type Attrs struct {
	A  chan int        `xml:"a,attr"`
	B  []func()        `xml:"b,attr"`
	C  []byte          `xml:"c,attr"`
	X  xml.Attr        `xml:",any,attr"`
	In RawBytes        `xml:",innerxml"`
	Cd chan int        `xml:",chardata"`
	M  map[string]int  `xml:"-"`
	Nm struct{ F int } `xml:"nm"`
}

type InnerString struct {
	S string `xml:",innerxml"`
}

type BadComment struct {
	C int `xml:",comment"`
}

type Embedded struct {
	*Attrs
	Ch chan bool
}

type Iface interface{ M() }

func indent() {
	xml.MarshalIndent(Attrs{}, "", " ")
	xml.MarshalIndent(&Attrs{}, "", " ")
	xml.Marshal(InnerString{})
	xml.Marshal(BadComment{})
	xml.Marshal(Embedded{})
	xml.Marshal([]Iface{})
	xml.Marshal([3]func(){})
	xml.Marshal([]byte{})
	json.MarshalIndent(Embedded{}, "", " ")
	json.MarshalIndent(map[Iface]int{}, "", " ")
	(*json.Encoder)(nil).Encode(func() {})
}

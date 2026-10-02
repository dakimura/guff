// Package protogen imports the deprecated github.com/golang/protobuf/proto
// three ways. SA1019 stays silent only in protoc-gen-go's own output.
package protogen

import "github.com/golang/protobuf/proto"

var _ = proto.Marshal

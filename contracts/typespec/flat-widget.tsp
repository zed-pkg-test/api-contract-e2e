import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace ZedFlatPeer;

@id("FlatWidget")
model FlatWidget {
  name: string;
  enabled: boolean;
}

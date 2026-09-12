import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace FlatVisualizationQuery;

@id("VisualizationQueryV1")
model VisualizationQueryV1 {
  @minValue(1)
  @maxValue(1)
  version: integer;

  @minLength(1)
  @maxLength(128)
  dataset_id: string;

  @minLength(1)
  @maxLength(128)
  view_id: string;

  @minValue(1)
  @maxValue(500)
  limit: integer;

  include_archived?: boolean;
}

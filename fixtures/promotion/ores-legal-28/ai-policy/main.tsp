import "@typespec/json-schema";
using TypeSpec.JsonSchema;

@jsonSchema
namespace OresLegal.AiPolicy;

@extension("additionalProperties", false)
model AiPolicySubject {
  @minLength(1) @maxLength(192) subject_ref: string;
  kind: "human" | "service" | "agent";
}

@extension("additionalProperties", false)
model AiResourceContext {
  @minLength(1) @maxLength(192) resource_ref: string;
  resource_kind: "repository" | "source_code" | "document" | "dataset" | "message" | "artifact" | "tool";
  classification: "public" | "internal" | "confidential" | "restricted";
  @minLength(1) @maxLength(128) tenant_ref?: string;
  contains_pii: boolean;
  contains_secrets: boolean;
  legal_hold: boolean;
}

@extension("additionalProperties", false)
model AiExecutionContext {
  action: "read" | "summarize" | "generate" | "modify" | "execute" | "publish" | "export";
  @minLength(1) @maxLength(128) provider_class: string;
  @minLength(1) @maxLength(128) model_class: string;
  environment: "local" | "sandbox" | "ci" | "server" | "external_provider";
  training_use: "prohibited" | "provider_default" | "explicitly_allowed";
  retention: "none" | "ephemeral" | "bounded" | "provider_default";
}

@extension("additionalProperties", false)
model AiPolicyConstraint {
  kind: "human_review" | "tool_allowlist" | "provider_allowlist" | "no_external_export" | "no_training" | "ephemeral_retention" | "sandbox_required" | "redaction_required";
  @minLength(1) @maxLength(192) value?: string;
}

@extension("additionalProperties", false)
model AiPolicyEvaluationRequest {
  @pattern("^[A-Za-z0-9_-]{16,128}$") request_id: string;
  subject: AiPolicySubject;
  resource: AiResourceContext;
  execution: AiExecutionContext;
  @pattern("^[a-f0-9]{64}$") policy_bundle_sha256: string;
  @minValue(1) @maxValue(9007199254740991) evaluated_at: integer;
}

@extension("additionalProperties", false)
model AiPolicyDecision {
  @pattern("^[A-Za-z0-9_-]{16,128}$") decision_id: string;
  @minLength(1) @maxLength(128) policy_version: string;
  result: "allow" | "deny" | "require_review" | "allow_with_constraints";
  constraints: AiPolicyConstraint[];
  @pattern("^[a-f0-9]{64}$") request_sha256: string;
  @pattern("^[a-f0-9]{64}$") compiled_policy_sha256: string;
  @pattern("^[a-f0-9]{64}$") source_policy_sha256: string;
  @minValue(1) @maxValue(9007199254740991) decided_at: integer;
  @minValue(1) @maxValue(9007199254740991) expires_at?: integer;
}

@extension("additionalProperties", false)
model AiPolicyReviewReceipt {
  @pattern("^[A-Za-z0-9_-]{16,128}$") review_id: string;
  @pattern("^[A-Za-z0-9_-]{16,128}$") decision_id: string;
  @minLength(1) @maxLength(192) reviewer_ref: string;
  result: "approved" | "rejected";
  @pattern("^[a-f0-9]{64}$") reviewed_resource_sha256: string;
  @minValue(1) @maxValue(9007199254740991) reviewed_at: integer;
}

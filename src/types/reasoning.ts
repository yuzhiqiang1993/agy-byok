export type ReasoningLevel = "off" | "low" | "medium" | "high" | "x_high" | "max" | "auto";
export type ConfigurableReasoningLevel = "low" | "medium" | "high" | "x_high" | "max";

export type ReasoningMapping =
  | { kind: "effort"; value: string }
  | { kind: "budget_tokens"; value: number }
  | { kind: "native_level"; value: string };

export type ReasoningLevel = "off" | "low" | "medium" | "high" | "x_high" | "max" | "adaptive" | "auto";
export type ConfigurableReasoningLevel = "low" | "medium" | "high" | "x_high" | "max" | "adaptive";

export interface ThinkingBudgetConfig {
  thinkingBudget: number | null;
  minThinkingBudget: number | null;
}

export type ReasoningMapping =
  | { kind: "disabled" }
  | { kind: "adaptive" }
  | { kind: "effort"; value: string }
  | { kind: "budget_tokens"; value: number }
  | { kind: "native_level"; value: string };

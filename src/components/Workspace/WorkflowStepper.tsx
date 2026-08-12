import { Check, ChevronRight, Circle } from "lucide-react";

export type WorkflowStep = {
  id: string;
  label: string;
  description: string;
  status: "complete" | "current" | "pending";
};

export function WorkflowStepper({ steps }: { steps: WorkflowStep[] }) {
  return (
    <ol className="workflow-stepper" aria-label="改写流程">
      {steps.map((step) => (
        <li key={step.id} className={`workflow-step ${step.status}`} aria-current={step.status === "current" ? "step" : undefined}>
          <span className="workflow-step-marker" aria-hidden="true">
            {step.status === "complete" ? <Check size={13} /> : step.status === "current" ? <ChevronRight size={13} /> : <Circle size={10} />}
          </span>
          <span><strong>{step.label}</strong><small>{step.description}</small></span>
        </li>
      ))}
    </ol>
  );
}

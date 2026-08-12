import type { ReactNode } from "react";
import type { WorkflowStep } from "./WorkflowStepper";
import { WorkflowStepper } from "./WorkflowStepper";

type TaskCenterProps = {
  batchControl: ReactNode;
  steps: WorkflowStep[];
  suggestedAction: ReactNode;
  runningTask?: ReactNode;
  automaticActions: ReactNode;
  manualActions: ReactNode;
  experimentAction: ReactNode;
  estimate?: ReactNode;
};

export function TaskCenter(props: TaskCenterProps) {
  return (
    <aside className="task-center" aria-label="任务中心">
      <header className="task-center-heading"><h2>任务中心</h2></header>
      <div className="task-center-scroll">
        <section className="task-center-section task-batch-control">{props.batchControl}</section>
        <section className="task-center-section"><WorkflowStepper steps={props.steps} /></section>
        <section className="task-center-section task-suggestion">
          <span className="task-section-label">建议下一步</span>
          {props.suggestedAction}
        </section>
        {props.runningTask && <section className="task-center-section task-running-surface">{props.runningTask}</section>}
        <section className="task-center-section task-action-section">
          <span className="task-section-label">自动流程</span>
          <div className="task-action-stack">{props.automaticActions}</div>
        </section>
        <section className="task-center-section task-action-section">
          <span className="task-section-label">单步处理</span>
          <div className="task-action-grid">{props.manualActions}</div>
        </section>
        <section className="task-center-section task-action-section">
          <span className="task-section-label">实验</span>
          <div className="task-action-stack">{props.experimentAction}</div>
        </section>
        {props.estimate && <div className="task-center-estimate">{props.estimate}</div>}
      </div>
    </aside>
  );
}

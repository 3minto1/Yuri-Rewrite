import type { ReactNode } from "react";

type WorkspaceDashboardProps = {
  title: string;
  meta: string;
  modelName: string;
  chapters: ReactNode;
  taskCenter: ReactNode;
};

export function WorkspaceDashboard({ title, meta, modelName, chapters, taskCenter }: WorkspaceDashboardProps) {
  return (
    <div className="workspace-dashboard">
      <header className="workspace-command-header topbar">
        <div><h1>{title}</h1><p>{meta}</p></div>
        <div className="workspace-model-summary"><span>当前改写模型</span><strong>{modelName}</strong></div>
      </header>
      <div className="workspace-dashboard-body">
        <div className="workspace-chapter-surface">{chapters}</div>
        {taskCenter}
      </div>
    </div>
  );
}
